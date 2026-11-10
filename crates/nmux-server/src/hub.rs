//! The heart of the server: the model, the running panes, and every change to
//! either. Clients send [`Op`]s; everything they need to know comes back as a
//! reply or a broadcast [`Event`].

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nmux_core::detect;
use nmux_core::validate;
use nmux_core::{AgentState, Created, Dir, PaneId, PaneKind, PaneSpec, PaneStatus, State};
use nmux_proto::{Event, Op, Reply, Snapshot};
use tokio::sync::{broadcast, watch};

use crate::cloud_id::CloudIdSniffer;
use crate::launch::{self, ClaudeOptions};
use crate::persist;
use crate::pty::PtyProcess;
use crate::screen::{Screen, DEFAULT_COLS, DEFAULT_ROWS};
use crate::transcripts::{self, Transcript};

pub struct Config {
    /// Where `state.json` lives.
    pub data_dir: PathBuf,
    /// Environment panes are started from.
    pub env: BTreeMap<String, String>,
    /// Claude Code's config dir, to check whether a conversation can be resumed.
    pub claude_dir: Option<PathBuf>,
    /// The Electron app's `sessions.json`, imported on first run.
    pub legacy_sessions: Option<PathBuf>,
}

struct Runtime {
    screen: Screen,
    process: Option<PtyProcess>,
    /// Which start of the pane this is; callbacks from an older one are ignored.
    generation: u64,
    sniffer: Option<CloudIdSniffer>,
    /// Output arrived since detection last looked.
    detect_dirty: bool,
}

struct Inner {
    state: State,
    runtimes: HashMap<PaneId, Runtime>,
    generation: u64,
    window_visible: bool,
    save_dirty: bool,
    last_save: Instant,
}

pub struct Hub {
    inner: Mutex<Inner>,
    events: broadcast::Sender<Arc<Event>>,
    config: Config,
    pub shutdown: watch::Sender<bool>,
}

struct Start {
    resume: bool,
    teleport: Option<String>,
    notice: Option<String>,
}

/// Processes to drop once the lock is released. Closing a pseudo-console can
/// wait for its reader, and the reader needs the lock.
type Graveyard = Vec<PtyProcess>;

const CURSOR_QUERY: &str = "\x1b[6n";
const SAVE_EVERY: Duration = Duration::from_millis(500);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Hub {
    pub fn new(config: Config) -> Arc<Hub> {
        let mut imported = false;
        let state = persist::load(&config.data_dir).unwrap_or_else(|| {
            let legacy = config.legacy_sessions.as_deref().and_then(persist::import_legacy);
            imported = legacy.is_some();
            legacy.unwrap_or_default()
        });
        let (events, _) = broadcast::channel(4096);
        Arc::new(Hub {
            inner: Mutex::new(Inner {
                state,
                runtimes: HashMap::new(),
                generation: 0,
                window_visible: true,
                save_dirty: imported,
                last_save: Instant::now(),
            }),
            events,
            config,
            shutdown: watch::Sender::new(false),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock must not take every pane down with it.
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

    /// Run one client request.
    pub fn handle(self: &Arc<Self>, op: Op) -> Result<Reply, String> {
        let mut graveyard = Graveyard::new();
        let result = {
            let mut inner = self.lock();
            self.handle_locked(&mut inner, op, &mut graveyard)
        };
        drop(graveyard);
        result
    }

    fn handle_locked(self: &Arc<Self>, inner: &mut Inner, op: Op, graveyard: &mut Graveyard) -> Result<Reply, String> {
        let e = |err: nmux_core::ModelError| err.to_string();
        match op {
            Op::Hello { .. } => Err("already connected".into()),
            Op::GetState => Ok(Reply::State { state: inner.state.clone() }),

            Op::CreateWorkspace { name, cwd, pane } => {
                let cwd = validate::cwd(&cwd).map_err(|r| r.0)?;
                if !launch::is_dir(&cwd) {
                    return Err(format!("folder does not exist: {cwd}"));
                }
                let name = name.as_deref().map(validate::name).transpose().map_err(|r| r.0)?;
                let spec = self.checked_spec(&pane)?;
                let created = inner.state.create_workspace(name, cwd, spec, now_ms());
                Ok(self.started(inner, created))
            }
            Op::CreateTab { workspace, name, pane } => {
                let name = name.as_deref().filter(|n| !n.trim().is_empty()).map(validate::name).transpose().map_err(|r| r.0)?;
                let spec = self.checked_spec(&pane)?;
                let created = inner.state.create_tab(workspace, name, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }
            Op::SplitPane { target, dir, pane } => {
                let spec = self.checked_spec(&pane)?;
                let created = inner.state.split_pane(target, dir, spec, now_ms()).map_err(e)?;
                Ok(self.started(inner, created))
            }

            Op::ClosePane { pane } => {
                let closed = inner.state.close_pane(pane).map_err(e)?;
                Ok(self.closed(inner, closed, graveyard))
            }
            Op::CloseTab { tab } => {
                let closed = inner.state.close_tab(tab).map_err(e)?;
                Ok(self.closed(inner, closed, graveyard))
            }
            Op::CloseWorkspace { workspace } => {
                let closed = inner.state.close_workspace(workspace).map_err(e)?;
                Ok(self.closed(inner, closed, graveyard))
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
            Op::Resize { pane, cols, rows } => {
                if !(2..=1000).contains(&cols) || !(1..=1000).contains(&rows) {
                    return Err("bad terminal size".into());
                }
                if let Some(rt) = inner.runtimes.get_mut(&pane) {
                    if rt.screen.resize(cols, rows) {
                        if let Some(process) = &rt.process {
                            process.resize(cols, rows);
                        }
                    }
                }
                Ok(Reply::Done)
            }

            Op::SetWindowVisible { visible } => {
                inner.window_visible = visible;
                if visible {
                    // Whatever finished in the tab on screen has now been seen.
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
                for rt in inner.runtimes.values_mut() {
                    graveyard.extend(rt.process.take());
                }
                let panes: Vec<PaneId> = inner.state.panes.keys().copied().collect();
                for pane in panes {
                    inner.state.set_exited(pane, None);
                }
                let _ = persist::save(&self.config.data_dir, &inner.state);
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

    fn checked_spec(&self, raw: &PaneSpec) -> Result<PaneSpec, String> {
        let spec = validate::pane_spec(raw).map_err(|r| r.0)?;
        if let Some(cwd) = &spec.cwd {
            if !launch::is_dir(cwd) {
                return Err(format!("folder does not exist: {cwd}"));
            }
        }
        Ok(spec)
    }

    /// A pane was just added to the model: give it its ids and start it.
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

    fn closed(&self, inner: &mut Inner, closed: nmux_core::Closed, graveyard: &mut Graveyard) -> Reply {
        for pane in &closed.panes {
            if let Some(mut rt) = inner.runtimes.remove(pane) {
                graveyard.extend(rt.process.take());
            }
        }
        self.changed(inner);
        Reply::Closed { closed }
    }

    /// Resume a Claude conversation, re-run a cloud launcher, or start a new shell.
    ///
    /// `claude --resume` exits with "No conversation found" when the transcript
    /// has no messages, so that case starts a fresh conversation instead of
    /// dying with a blank screen.
    fn restart(self: &Arc<Self>, inner: &mut Inner, id: PaneId, graveyard: &mut Graveyard) -> Result<(), String> {
        let pane = inner.state.panes.get_mut(&id).ok_or("no such pane")?;
        if pane.status == PaneStatus::Running {
            return Ok(());
        }
        let mut start = Start { resume: pane.kind == PaneKind::Claude, teleport: None, notice: None };
        match pane.kind {
            PaneKind::Claude => {
                let transcript = match (&pane.claude_session_id, &self.config.claude_dir) {
                    (None, _) => Transcript::Missing,
                    (Some(session), Some(dir)) => transcripts::state(dir, &pane.cwd, session),
                    (Some(_), None) => Transcript::Unknown,
                };
                if matches!(transcript, Transcript::Missing | Transcript::Empty) {
                    start.resume = false;
                    // Claude refuses `--session-id` for an id that already has a
                    // file, even one without messages, so that needs a new id.
                    if transcript == Transcript::Empty || pane.claude_session_id.is_none() {
                        pane.claude_session_id = Some(uuid::Uuid::new_v4().to_string());
                    }
                    // A teleported conversation is only saved locally once you
                    // send something; until then, bring it in from the cloud again.
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
        let env = launch::pane_env(&self.config.env, id);
        let command = match pane.kind {
            PaneKind::Claude => launch::claude(
                ClaudeOptions {
                    session_id: pane.claude_session_id.as_deref().unwrap_or_default(),
                    resume: start.resume,
                    extra_args: &pane.extra_args,
                    skip_permissions: pane.skip_permissions,
                    teleport: start.teleport.as_deref(),
                },
                &env,
            ),
            PaneKind::Cloud => launch::cloud(pane.cloud_task.as_deref().unwrap_or_default(), &pane.extra_args, &env),
            PaneKind::Shell => launch::shell(&env),
        };
        let kind = pane.kind;
        let cwd = pane.cwd.clone();

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
        // A restart begins on a clean terminal, here and in every attached window.
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

    /// Put text on a pane's screen as if the pane had printed it.
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
        // ConPTY asks where the cursor is when it starts and waits for the
        // answer. A terminal would reply, but a pane has to start with no
        // window attached, so the server answers and keeps the question to
        // itself (otherwise an attached terminal would answer a second time).
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

    /// Periodic work: read agent states off the screens, and save.
    pub fn tick(&self) {
        let to_save = {
            let mut inner = self.lock();
            let inner = &mut *inner;
            let mut changed = false;
            for (id, rt) in inner.runtimes.iter_mut() {
                if !std::mem::take(&mut rt.detect_dirty) || rt.process.is_none() {
                    continue;
                }
                let Some(pane) = inner.state.panes.get(id) else { continue };
                if pane.kind != PaneKind::Claude {
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
