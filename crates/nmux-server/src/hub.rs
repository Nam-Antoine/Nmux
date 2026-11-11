use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nmux_core::detect;
use nmux_core::validate;
use nmux_core::{AgentState, Created, Dir, MachineId, MachineStatus, PaneId, PaneKind, PaneSpec, PaneStatus, ShellClaude, State, WorkspaceId};
use nmux_proto::{Event, Op, Reply, Snapshot};
use tokio::sync::{broadcast, watch};

use crate::cloud_id::CloudIdSniffer;
use crate::files;
use crate::git;
use crate::ports::{self, Forwards};
use crate::launch::{self, ClaudeOptions};
use crate::persist;
use crate::pty::PtyProcess;
use crate::remote;
use crate::sshconfig;
use crate::screen::{Screen, DEFAULT_COLS, DEFAULT_ROWS};
use crate::transcripts::{self, Transcript};

pub struct Config {
    pub data_dir: PathBuf,
    pub env: BTreeMap<String, String>,
    pub claude_dir: Option<PathBuf>,
}

struct Runtime {
    screen: Screen,
    process: Option<PtyProcess>,
    generation: u64,
    sniffer: Option<CloudIdSniffer>,
    detect_dirty: bool,
}

struct Inner {
    state: State,
    runtimes: HashMap<PaneId, Runtime>,
    generation: u64,
    window_visible: bool,
    save_dirty: bool,
    last_save: Instant,
    ticks: u32,
}

pub struct Hub {
    inner: Mutex<Inner>,
    events: broadcast::Sender<Arc<Event>>,
    config: Config,
    processes: Mutex<sysinfo::System>,
    forwards: Forwards,
    pub shutdown: watch::Sender<bool>,
}

struct Start {
    resume: bool,
    teleport: Option<String>,
    notice: Option<String>,
}

type Graveyard = Vec<PtyProcess>;

const CURSOR_QUERY: &str = "\x1b[6n";
const SAVE_EVERY: Duration = Duration::from_millis(500);
const SHELL_CHECK_TICKS: u32 = 8;

fn remote_sessions(state: &State, owner: &str) -> HashMap<PaneId, (String, String)> {
    state
        .panes
        .values()
        .filter_map(|p| {
            let machine: MachineId = p.machine?;
            let target = state.machine(machine)?.target.clone();
            Some((p.id, (target, remote::tmux_session(owner, p.id, p.created_at))))
        })
        .collect()
}

fn default_machine_label(target: &str) -> String {
    let rest = target.strip_prefix("ssh://").unwrap_or(target);
    let rest = rest.rsplit('@').next().unwrap_or(rest);
    let host = rest.split(['/', ':']).next().unwrap_or(rest);
    if host.is_empty() { target.to_string() } else { host.to_string() }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Hub {
    pub fn new(config: Config) -> Arc<Hub> {
        let (state, were_running) = persist::load(&config.data_dir).unwrap_or_default();
        let (events, _) = broadcast::channel(4096);
        let forward_events = events.clone();
        let forwards = Forwards::load(
            &config.data_dir,
            Arc::new(move |forwards| {
                let _ = forward_events.send(Arc::new(Event::Forwards { forwards }));
            }),
        );
        let hub = Arc::new(Hub {
            forwards,
            inner: Mutex::new(Inner {
                state,
                runtimes: HashMap::new(),
                generation: 0,
                window_visible: true,
                save_dirty: false,
                last_save: Instant::now(),
                ticks: 0,
            }),
            events,
            config,
            processes: Mutex::new(sysinfo::System::new()),
            shutdown: watch::Sender::new(false),
        });
        for pane in were_running {
            let _ = hub.handle(Op::RestartPane { pane });
        }
        hub
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Arc<Event>> {
        self.events.subscribe()
    }

    pub fn state(&self) -> State {
        self.lock().state.clone()
    }

    fn emit(&self, event: Event) {
        let _ = self.events.send(Arc::new(event));
    }

    fn changed(&self, inner: &mut Inner) {
        inner.save_dirty = true;
        self.emit(Event::State { state: inner.state.clone() });
    }

    pub fn handle(self: &Arc<Self>, op: Op) -> Result<Reply, String> {
        let op = match self.handle_unlocked(op)? {
            Ok(reply) => return Ok(reply),
            Err(op) => op,
        };
        let mut graveyard = Graveyard::new();
        let result = {
            let mut inner = self.lock();
            self.handle_locked(&mut inner, op, &mut graveyard)
        };
        drop(graveyard);
        result
    }

    fn workspace_place(&self, workspace: WorkspaceId) -> Result<(String, Option<String>), String> {
        let inner = self.lock();
        let ws = inner.state.workspaces.iter().find(|w| w.id == workspace).ok_or("no such workspace")?;
        Ok((ws.cwd.clone(), ws.machine.and_then(|m| inner.state.machine(m)).map(|m| m.target.clone())))
    }

    fn machine_target(&self, machine: MachineId) -> Result<String, String> {
        Ok(self.lock().state.machine(machine).ok_or("no such machine")?.target.clone())
    }

    fn workspace_roots(&self) -> Vec<String> {
        self.lock().state.workspaces.iter().map(|w| w.cwd.clone()).collect()
    }

    fn handle_unlocked(&self, op: Op) -> Result<Result<Reply, Op>, String> {
        let forwards = |forwards: Vec<nmux_proto::Forward>| {
            self.emit(Event::Forwards { forwards: forwards.clone() });
            Reply::Forwards { forwards }
        };
        Ok(Ok(match op {
            Op::ListDir { path, machine } => {
                let (path, entries) = match machine {
                    Some(machine) => remote::list_dir(&self.machine_target(machine)?, &validate::remote_cwd(&path).map_err(|r| r.0)?, &self.config.env)?,
                    None => files::list_dir(&path, &self.workspace_roots())?,
                };
                Reply::Dir { path, entries }
            }
            Op::ReadFile { path, machine } => Reply::File {
                file: match machine {
                    Some(machine) => remote::read_file(&self.machine_target(machine)?, &validate::remote_cwd(&path).map_err(|r| r.0)?, &self.config.env)?,
                    None => files::read_file(&path, &self.workspace_roots())?,
                },
            },
            Op::CheckPath { path } => {
                Reply::Path { path: files::display(&files::resolve_inside(&path, &self.workspace_roots())?) }
            }
            Op::PasteImage { pane, data, .. } => {
                if !self.lock().runtimes.get(&pane).is_some_and(|rt| rt.process.is_some()) {
                    return Err("that pane is not running".into());
                }
                let path = files::stage_pasted_image(&self.config.data_dir, &data)?;
                let target = {
                    let inner = self.lock();
                    inner.state.panes.get(&pane).and_then(|p| p.machine).and_then(|m| inner.state.machine(m)).map(|m| m.target.clone())
                };
                let text = match target {
                    Some(target) => {
                        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
                        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        remote::upload(&target, &name, &bytes, &self.config.env)?
                    }
                    None => files::display(&path),
                };
                let inner = self.lock();
                if let Some(rt) = inner.runtimes.get(&pane) {
                    if let Some(process) = &rt.process {
                        if rt.screen.bracketed_paste() {
                            process.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                        } else {
                            process.write(text.as_bytes());
                        }
                    }
                }
                Reply::Path { path: text }
            }
            Op::ListPorts => {
                let roots: HashMap<u32, PaneId> = self
                    .lock()
                    .runtimes
                    .iter()
                    .filter_map(|(id, rt)| rt.process.as_ref().and_then(|p| p.pid).map(|pid| (pid, *id)))
                    .collect();
                Reply::Ports { ports: ports::listening(&roots) }
            }
            Op::ListMachinePorts { machine } => {
                let ports = remote::listening(&self.machine_target(machine)?, &self.config.env)?
                    .into_iter()
                    .map(|(address, port, process)| nmux_proto::ListeningPort { port, address, pid: 0, process, pane: None })
                    .collect();
                Reply::Ports { ports }
            }
            Op::GitChanges { workspace } => {
                let (cwd, target) = self.workspace_place(workspace)?;
                Reply::Changes { changes: git::Repo { cwd: &cwd, target: target.as_deref(), env: &self.config.env }.changes()? }
            }
            Op::GitDiff { workspace, path, untracked } => {
                let (cwd, target) = self.workspace_place(workspace)?;
                let (diff, truncated) = git::Repo { cwd: &cwd, target: target.as_deref(), env: &self.config.env }.diff(&path, untracked)?;
                Reply::Diff { diff, truncated }
            }
            Op::ListForwards => Reply::Forwards { forwards: self.forwards.list() },
            Op::ListSshHosts => Reply::SshHosts { hosts: sshconfig::hosts(&self.config.env) },
            Op::StartForward { host, remote_port, local_port } => forwards(self.forwards.start(host.trim(), remote_port, local_port)?),
            Op::StopForward { id } => forwards(self.forwards.stop(id)?),
            Op::RemoveForward { id } => forwards(self.forwards.remove(id)?),
            Op::CheckMachine { machine } => {
                let target = {
                    let mut inner = self.lock();
                    let target = inner.state.machine(machine).ok_or("no such machine")?.target.clone();
                    if inner.state.set_machine_status(machine, MachineStatus::Checking, None, false, false) {
                        self.emit(Event::State { state: inner.state.clone() });
                    }
                    target
                };
                let found = remote::probe(&target, &self.config.env);
                let mut inner = self.lock();
                if inner.state.machine(machine).is_some_and(|m| m.target == target)
                    && inner.state.set_machine_status(machine, found.status, found.detail, found.tmux, found.claude)
                {
                    self.emit(Event::State { state: inner.state.clone() });
                }
                Reply::Done
            }
            other => return Ok(Err(other)),
        }))
    }

    fn handle_locked(self: &Arc<Self>, inner: &mut Inner, op: Op, graveyard: &mut Graveyard) -> Result<Reply, String> {
        let e = |err: nmux_core::ModelError| err.to_string();
        match op {
            Op::Hello { .. } => Err("already connected".into()),
            Op::ListDir { .. }
            | Op::ReadFile { .. }
            | Op::CheckPath { .. }
            | Op::PasteImage { .. }
            | Op::ListPorts
            | Op::ListMachinePorts { .. }
            | Op::GitChanges { .. }
            | Op::GitDiff { .. }
            | Op::ListForwards
            | Op::ListSshHosts
            | Op::StartForward { .. }
            | Op::StopForward { .. }
            | Op::RemoveForward { .. }
            | Op::CheckMachine { .. } => Err("handled elsewhere".into()),
            Op::GetState => Ok(Reply::State { state: inner.state.clone() }),

            Op::CreateWorkspace { name, cwd, pane, machine: Some(machine) } => {
                let cwd = validate::remote_cwd(&cwd).map_err(|r| r.0)?;
                let name = name.as_deref().map(validate::name).transpose().map_err(|r| r.0)?;
                let name = name.or_else(|| (cwd.trim_end_matches('/') == "~").then(|| "home".to_string()));
                let spec = self.checked_spec(&pane, true)?;
                let created = inner.state.create_remote_workspace(machine, name, cwd, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }
            Op::CreateWorkspace { name, cwd, pane, machine: None } => {
                let cwd = validate::cwd(&cwd).map_err(|r| r.0)?;
                if !launch::is_dir(&cwd) {
                    return Err(format!("folder does not exist: {cwd}"));
                }
                let name = name.as_deref().map(validate::name).transpose().map_err(|r| r.0)?;
                let spec = self.checked_spec(&pane, false)?;
                let created = inner.state.create_workspace(name, cwd, spec, now_ms());
                Ok(self.started(inner, created))
            }
            Op::AddMachine { label, target } => {
                let target = validate::ssh_target(&target).map_err(|r| r.0)?;
                let label = match label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
                    Some(label) => validate::name(label).map_err(|r| r.0)?,
                    None => default_machine_label(&target),
                };
                inner.state.add_machine(label, target).map_err(e)?;
                self.changed(inner);
                Ok(Reply::Done)
            }
            Op::RenameMachine { machine, label } => {
                let label = validate::name(&label).map_err(|r| r.0)?;
                self.done(inner, |s| s.rename_machine(machine, label))
            }
            Op::RemoveMachine { machine } => self.done(inner, |s| s.remove_machine(machine)),
            Op::CreateTab { workspace, name, pane } => {
                let name = name.as_deref().filter(|n| !n.trim().is_empty()).map(validate::name).transpose().map_err(|r| r.0)?;
                let remote = inner.state.workspace(workspace).is_some_and(|w| w.machine.is_some());
                let spec = self.checked_spec(&pane, remote)?;
                let created = inner.state.create_tab(workspace, name, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }
            Op::SplitPane { target, dir, pane } => {
                let remote = inner.state.panes.get(&target).is_some_and(|p| p.machine.is_some());
                let spec = self.checked_spec(&pane, remote)?;
                let created = inner.state.split_pane(target, dir, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }

            Op::ClosePane { pane } => {
                let far = remote_sessions(&inner.state, &remote::owner(&self.config.env));
                let closed = inner.state.close_pane(pane).map_err(e)?;
                Ok(self.closed(inner, closed, far, graveyard))
            }
            Op::StopPane { pane } => {
                let rt = inner.runtimes.get_mut(&pane).ok_or("that pane is not running")?;
                graveyard.extend(rt.process.take());
                inner.state.set_exited(pane, None);
                self.changed(inner);
                Ok(Reply::Done)
            }
            Op::CloseTab { tab } => {
                let far = remote_sessions(&inner.state, &remote::owner(&self.config.env));
                let closed = inner.state.close_tab(tab).map_err(e)?;
                Ok(self.closed(inner, closed, far, graveyard))
            }
            Op::CloseWorkspace { workspace } => {
                let far = remote_sessions(&inner.state, &remote::owner(&self.config.env));
                let closed = inner.state.close_workspace(workspace).map_err(e)?;
                Ok(self.closed(inner, closed, far, graveyard))
            }

            Op::FocusPane { pane } => self.done(inner, |s| s.focus_pane(pane)),
            Op::SelectTab { tab } => self.done(inner, |s| s.select_tab(tab)),
            Op::SelectWorkspace { workspace } => self.done(inner, |s| s.select_workspace(workspace)),
            Op::FocusSide { pane, side } => {
                let next = inner.state.focus_side(pane, side).map_err(e)?;
                if next.is_some() {
                    self.changed(inner);
                }
                Ok(Reply::Pane { pane: next })
            }
            Op::SwapPanes { a, b } => self.done(inner, |s| s.swap_panes(a, b)),
            Op::MovePane { pane, target, side } => self.done(inner, |s| s.move_pane(pane, target, side)),
            Op::SetRatio { tab, path, ratio } => self.done(inner, |s| s.set_ratio(tab, &path, ratio)),
            Op::ResizeSplit { pane, side, delta } => {
                if !delta.is_finite() {
                    return Err("bad resize".into());
                }
                let moved = inner.state.resize_pane(pane, side, delta.clamp(-0.5, 0.5)).map_err(e)?;
                if moved {
                    self.changed(inner);
                }
                Ok(Reply::Flag { value: moved })
            }
            Op::ToggleZoom { pane } => {
                let zoomed = inner.state.toggle_zoom(pane).map_err(e)?;
                self.changed(inner);
                Ok(Reply::Flag { value: zoomed })
            }

            Op::RenamePane { pane, name } => {
                let name = validate::name(&name).map_err(|r| r.0)?;
                self.done(inner, |s| s.rename_pane(pane, name))
            }
            Op::RenameTab { tab, name } => {
                let name = name.as_deref().filter(|n| !n.trim().is_empty()).map(validate::name).transpose().map_err(|r| r.0)?;
                self.done(inner, |s| s.rename_tab(tab, name))
            }
            Op::RenameWorkspace { workspace, name } => {
                let name = validate::name(&name).map_err(|r| r.0)?;
                self.done(inner, |s| s.rename_workspace(workspace, name))
            }

            Op::RestartPane { pane } => {
                self.restart(inner, pane, graveyard)?;
                Ok(Reply::Done)
            }
            Op::TeleportPane { pane } => {
                let source = inner.state.panes.get(&pane).ok_or("no such pane")?;
                let id = source.cloud_session_id.clone().ok_or("this pane has no cloud session yet")?;
                let mut spec = PaneSpec::new(PaneKind::Claude);
                spec.name = Some(validate::name(&format!("{} (teleported)", truncate_chars(&source.name, 60))).map_err(|r| r.0)?);
                spec.cwd = Some(source.cwd.clone());
                spec.teleport = Some(validate::cloud_session_id(&id).map_err(|r| r.0)?);
                let created = inner.state.split_pane(pane, Dir::Right, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }

            Op::Attach { pane } => {
                if !inner.state.panes.contains_key(&pane) {
                    return Err("no such pane".into());
                }
                let snapshot = match inner.runtimes.get_mut(&pane) {
                    Some(rt) => {
                        let (cols, rows) = rt.screen.size();
                        Snapshot { data: rt.screen.snapshot(), seq: rt.screen.seq, cols, rows }
                    }
                    None => Snapshot { data: String::new(), seq: 0, cols: DEFAULT_COLS, rows: DEFAULT_ROWS },
                };
                Ok(Reply::Attached { snapshot })
            }
            Op::Write { pane, data } => {
                if let Some(process) = inner.runtimes.get(&pane).and_then(|rt| rt.process.as_ref()) {
                    process.write(data.as_bytes());
                }
                Ok(Reply::Done)
            }
            Op::SendText { pane, text } => {
                let rt = inner.runtimes.get(&pane).ok_or("no such pane")?;
                let process = rt.process.as_ref().ok_or("that pane is not running; resume it first")?;
                if rt.screen.bracketed_paste() {
                    process.write(format!("\x1b[200~{text}\x1b[201~").as_bytes());
                } else {
                    process.write(text.as_bytes());
                }
                Ok(Reply::Done)
            }
            Op::Resize { pane, cols, rows } => {
                if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
                    return Err("bad terminal size".into());
                }
                if !inner.state.panes.contains_key(&pane) {
                    return Err("no such pane".into());
                }
                let generation = inner.generation;
                let rt = inner.runtimes.entry(pane).or_insert_with(|| Runtime {
                    screen: Screen::default(),
                    process: None,
                    generation,
                    sniffer: None,
                    detect_dirty: false,
                });
                if rt.screen.resize(cols, rows) {
                    if let Some(process) = &rt.process {
                        process.resize(cols, rows);
                    }
                }
                Ok(Reply::Done)
            }

            Op::SetWindowVisible { visible } => {
                inner.window_visible = visible;
                if visible {
                    let in_view: Vec<PaneId> =
                        inner.state.panes.keys().copied().filter(|p| inner.state.is_in_view(*p)).collect();
                    let mut any = false;
                    for pane in in_view {
                        any |= inner.state.mark_seen(pane);
                    }
                    if any {
                        self.changed(inner);
                    }
                }
                Ok(Reply::Done)
            }
            Op::Shutdown => {
                self.forwards.stop_all();
                for rt in inner.runtimes.values_mut() {
                    graveyard.extend(rt.process.take());
                }
                let _ = persist::save(&self.config.data_dir, &inner.state);
                let panes: Vec<PaneId> = inner.state.panes.keys().copied().collect();
                for pane in panes {
                    inner.state.set_exited(pane, None);
                }
                inner.save_dirty = false;
                self.shutdown.send_replace(true);
                Ok(Reply::Done)
            }
        }
    }

    fn done(&self, inner: &mut Inner, change: impl FnOnce(&mut State) -> Result<(), nmux_core::ModelError>) -> Result<Reply, String> {
        change(&mut inner.state).map_err(|e| e.to_string())?;
        self.changed(inner);
        Ok(Reply::Done)
    }

    fn checked_spec(&self, raw: &PaneSpec, remote: bool) -> Result<PaneSpec, String> {
        if remote {
            if raw.kind == PaneKind::Cloud {
                return Err("cloud tasks start from this computer; open one in a local workspace".into());
            }
            if raw.cwd.is_some() || raw.teleport.is_some() {
                return Err("a pane on another machine opens in its workspace folder".into());
            }
        }
        let spec = validate::pane_spec(raw).map_err(|r| r.0)?;
        if let Some(cwd) = &spec.cwd {
            if !launch::is_dir(cwd) {
                return Err(format!("folder does not exist: {cwd}"));
            }
        }
        Ok(spec)
    }

    fn started(self: &Arc<Self>, inner: &mut Inner, created: Created) -> Reply {
        let teleport = inner.state.panes.get_mut(&created.pane).and_then(|pane| {
            if pane.kind == PaneKind::Claude {
                pane.claude_session_id = Some(uuid::Uuid::new_v4().to_string());
            }
            pane.teleported_from.clone()
        });
        self.spawn(inner, created.pane, Start { resume: false, teleport, notice: None });
        self.changed(inner);
        Reply::Created { created }
    }

    fn closed(
        &self,
        inner: &mut Inner,
        closed: nmux_core::Closed,
        far: HashMap<PaneId, (String, String)>,
        graveyard: &mut Graveyard,
    ) -> Reply {
        let mut by_target: HashMap<String, Vec<String>> = HashMap::new();
        for pane in &closed.panes {
            if let Some((target, session)) = far.get(pane) {
                by_target.entry(target.clone()).or_default().push(session.clone());
            }
        }
        for (target, sessions) in by_target {
            remote::end_sessions(target, sessions, self.config.env.clone());
        }
        for pane in &closed.panes {
            if let Some(mut rt) = inner.runtimes.remove(pane) {
                graveyard.extend(rt.process.take());
            }
        }
        self.changed(inner);
        Reply::Closed { closed }
    }

    fn restart(self: &Arc<Self>, inner: &mut Inner, id: PaneId, graveyard: &mut Graveyard) -> Result<(), String> {
        let pane = inner.state.panes.get_mut(&id).ok_or("no such pane")?;
        if pane.status == PaneStatus::Running {
            return Ok(());
        }
        let mut start = Start { resume: pane.kind == PaneKind::Claude, teleport: None, notice: None };
        match pane.kind {
            PaneKind::Claude if pane.machine.is_some() => {
                if pane.claude_session_id.is_none() {
                    pane.claude_session_id = Some(uuid::Uuid::new_v4().to_string());
                    start.resume = false;
                }
            }
            PaneKind::Claude => {
                let transcript = match (&pane.claude_session_id, &self.config.claude_dir) {
                    (None, _) => Transcript::Missing,
                    (Some(session), Some(dir)) => transcripts::state(dir, &pane.cwd, session),
                    (Some(_), None) => Transcript::Unknown,
                };
                if matches!(transcript, Transcript::Missing | Transcript::Empty) {
                    start.resume = false;
                    if transcript == Transcript::Empty || pane.claude_session_id.is_none() {
                        pane.claude_session_id = Some(uuid::Uuid::new_v4().to_string());
                    }
                    start.teleport = pane.teleported_from.clone();
                    if start.teleport.is_none() {
                        start.notice =
                            Some("Nothing to resume: the previous conversation had no messages, so this is a new one.".into());
                    }
                }
            }
            PaneKind::Cloud => {
                if pane.cloud_session_id.is_some() {
                    start.notice = Some("This asks for a new cloud session with the same task. To continue the existing one, use Teleport.".into());
                }
            }
            PaneKind::Shell => {}
        }
        if let Some(rt) = inner.runtimes.get_mut(&id) {
            graveyard.extend(rt.process.take());
        }
        self.spawn(inner, id, start);
        self.changed(inner);
        Ok(())
    }

    fn spawn(self: &Arc<Self>, inner: &mut Inner, id: PaneId, start: Start) {
        let Some(pane) = inner.state.panes.get(&id) else { return };
        let mut env = launch::pane_env(&self.config.env, id);
        env.insert("NMUX_DATA_DIR".into(), self.config.data_dir.to_string_lossy().into_owned());
        let far = pane.machine.map(|m| inner.state.machine(m).map(|m| m.target.clone()));
        let command = match (&far, pane.kind) {
            (Some(target), kind) => {
                let program = match kind {
                    PaneKind::Claude => remote::Program::Claude {
                        session_id: pane.claude_session_id.as_deref().unwrap_or_default(),
                        resume: start.resume,
                        extra_args: &pane.extra_args,
                        skip_permissions: pane.skip_permissions,
                    },
                    _ => remote::Program::Shell,
                };
                let session = remote::tmux_session(&remote::owner(&self.config.env), id, pane.created_at);
                remote::launch(target.as_deref().unwrap_or("removed-machine.invalid"), &remote::script(&pane.cwd, &session, &program), &env)
            }
            (None, PaneKind::Claude) => launch::claude(
                ClaudeOptions {
                    session_id: pane.claude_session_id.as_deref().unwrap_or_default(),
                    resume: start.resume,
                    extra_args: &pane.extra_args,
                    skip_permissions: pane.skip_permissions,
                    teleport: start.teleport.as_deref(),
                },
                &env,
            ),
            (None, PaneKind::Cloud) => launch::cloud(pane.cloud_task.as_deref().unwrap_or_default(), &pane.extra_args, &env),
            (None, PaneKind::Shell) => launch::shell(&env),
        };
        let kind = pane.kind;
        let cwd = if far.is_some() { self.config.data_dir.to_string_lossy().into_owned() } else { pane.cwd.clone() };

        inner.generation += 1;
        let generation = inner.generation;
        let rt = inner.runtimes.entry(id).or_insert_with(|| Runtime {
            screen: Screen::default(),
            process: None,
            generation,
            sniffer: None,
            detect_dirty: false,
        });
        rt.generation = generation;
        rt.sniffer = (kind == PaneKind::Cloud).then(CloudIdSniffer::default);
        rt.screen.title = None;
        if rt.screen.seq > 0 {
            self.print(id, rt, "\x1bc");
        }
        if let Some(notice) = &start.notice {
            self.print(id, rt, &format!("\x1b[2m[nmux] {notice}\x1b[0m\r\n"));
        }

        let (cols, rows) = rt.screen.size();
        let on_data = {
            let hub: Weak<Hub> = Arc::downgrade(self);
            move |bytes: &[u8]| {
                if let Some(hub) = hub.upgrade() {
                    hub.on_data(id, generation, bytes);
                }
            }
        };
        let on_exit = {
            let hub: Weak<Hub> = Arc::downgrade(self);
            move |code: Option<i32>| {
                if let Some(hub) = hub.upgrade() {
                    hub.on_exit(id, generation, code);
                }
            }
        };
        match PtyProcess::spawn(&command, &cwd, &env, cols, rows, on_data, on_exit) {
            Ok(process) => {
                rt.process = Some(process);
                inner.state.set_running(id);
            }
            Err(err) => {
                self.print(id, rt, &format!("\x1b[31m[nmux] {err:#}\x1b[0m\r\n"));
                inner.state.set_exited(id, None);
            }
        }
    }

    fn print(&self, id: PaneId, rt: &mut Runtime, text: &str) {
        let data = rt.screen.feed(text.as_bytes());
        self.emit(Event::Output { pane: id, seq: rt.screen.seq, data });
    }

    fn on_data(&self, id: PaneId, generation: u64, bytes: &[u8]) {
        let mut inner = self.lock();
        let inner = &mut *inner;
        let Some(rt) = inner.runtimes.get_mut(&id) else { return };
        if rt.generation != generation {
            return;
        }
        let mut data = rt.screen.feed(bytes);
        rt.detect_dirty = true;
        if data.contains(CURSOR_QUERY) {
            let (row, col) = rt.screen.cursor();
            if let Some(process) = &rt.process {
                for _ in 0..data.matches(CURSOR_QUERY).count() {
                    process.write(format!("\x1b[{row};{col}R").as_bytes());
                }
            }
            data = data.replace(CURSOR_QUERY, "");
        }
        let found = rt.sniffer.as_mut().and_then(|s| s.push(&data));
        let seq = rt.screen.seq;
        if found.is_some() {
            rt.sniffer = None;
        }
        if !data.is_empty() {
            self.emit(Event::Output { pane: id, seq, data });
        }
        if let (Some(found), Some(pane)) = (found, inner.state.panes.get_mut(&id)) {
            pane.cloud_session_id = Some(found.id);
            pane.cloud_url = Some(found.url);
            self.changed(inner);
        }
    }

    fn on_exit(&self, id: PaneId, generation: u64, code: Option<i32>) {
        let process = {
            let mut inner = self.lock();
            let inner = &mut *inner;
            let Some(rt) = inner.runtimes.get_mut(&id) else { return };
            if rt.generation != generation {
                return;
            }
            let process = rt.process.take();
            inner.state.set_exited(id, code);
            self.changed(inner);
            process
        };
        drop(process);
    }

    fn shell_claude(&self, process: &ports::ClaudeProcess) -> ShellClaude {
        let file = self.config.claude_dir.as_ref().map(|dir| dir.join("sessions").join(format!("{}.json", process.pid)));
        let session: Option<serde_json::Value> = file.and_then(|f| std::fs::read(f).ok()).and_then(|body| serde_json::from_slice(&body).ok());
        let text = |key: &str| session.as_ref().and_then(|s| s.get(key)?.as_str().map(str::to_string));
        ShellClaude {
            session_id: text("sessionId").filter(|id| !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
            cwd: text("cwd").and_then(|cwd| validate::cwd(&cwd).ok()),
            skip_permissions: process.skip_permissions,
        }
    }

    fn watch_shells(&self) {
        let roots: HashMap<u32, PaneId> = {
            let mut inner = self.lock();
            inner.ticks = inner.ticks.wrapping_add(1);
            if inner.ticks % SHELL_CHECK_TICKS != 0 {
                return;
            }
            inner
                .runtimes
                .iter()
                .filter(|(id, _)| inner.state.panes.get(id).is_some_and(|p| p.kind == PaneKind::Shell && p.machine.is_none()))
                .filter_map(|(id, rt)| rt.process.as_ref().and_then(|p| p.pid).map(|pid| (pid, *id)))
                .collect()
        };
        if roots.is_empty() {
            return;
        }
        let found = ports::running_claude(&mut self.processes.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), &roots);
        let found: HashMap<PaneId, ShellClaude> = found.into_iter().map(|(pane, process)| (pane, self.shell_claude(&process))).collect();
        let mut inner = self.lock();
        let inner = &mut *inner;
        let mut changed = false;
        for id in roots.values() {
            if inner.state.set_runs_claude(*id, found.get(id).cloned()) {
                changed = true;
                if let Some(rt) = inner.runtimes.get_mut(id) {
                    rt.detect_dirty = true;
                }
            }
        }
        if changed {
            self.changed(inner);
        }
    }

    pub fn tick(&self) {
        self.watch_shells();
        let to_save = {
            let mut inner = self.lock();
            let inner = &mut *inner;
            let mut changed = false;
            for (id, rt) in inner.runtimes.iter_mut() {
                if !std::mem::take(&mut rt.detect_dirty) || rt.process.is_none() {
                    continue;
                }
                let Some(pane) = inner.state.panes.get(id) else { continue };
                if pane.shown_kind() != PaneKind::Claude {
                    continue;
                }
                let Some(detected) = detect::classify(rt.screen.title.as_deref(), &rt.screen.text()) else { continue };
                let before = pane.agent;
                if inner.state.set_detected(*id, detected, inner.window_visible) {
                    changed = true;
                    let after = inner.state.panes[id].agent;
                    let unseen = !(inner.window_visible && inner.state.is_in_view(*id));
                    if after == AgentState::Done || (after == AgentState::Blocked && before != AgentState::Blocked && unseen) {
                        self.emit(Event::Attention { pane: *id, blocked: after == AgentState::Blocked });
                    }
                }
            }
            if changed {
                self.changed(inner);
            }
            if inner.save_dirty && inner.last_save.elapsed() >= SAVE_EVERY {
                inner.save_dirty = false;
                inner.last_save = Instant::now();
                Some(inner.state.clone())
            } else {
                None
            }
        };
        if let Some(state) = to_save {
            if let Err(err) = persist::save(&self.config.data_dir, &state) {
                eprintln!("[nmux] save failed: {err}");
            }
        }
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}
