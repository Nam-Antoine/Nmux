use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmux_core::PaneId;
use nmux_proto::{Forward, ForwardState, ListeningPort};
use serde::{Deserialize, Serialize};

pub(crate) fn quiet(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command.stdin(Stdio::null())
}

pub fn parse_ss(text: &str) -> Vec<(String, u16, u32)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let Some((address, port)) = cols[3].rsplit_once(':') else { continue };
        let Ok(port) = port.parse::<u16>() else { continue };
        let pid = line.split("pid=").nth(1).and_then(|r| r.split(|c: char| !c.is_ascii_digit()).next()).and_then(|p| p.parse().ok());
        out.push((address.trim_matches(['[', ']']).to_string(), port, pid.unwrap_or(0)));
    }
    out
}

#[cfg(not(windows))]
fn raw_listeners() -> Vec<(String, u16, u32)> {
    match quiet(Command::new("ss").arg("-Hltnp")).output() {
        Ok(output) => parse_ss(&String::from_utf8_lossy(&output.stdout)),
        Err(_) => Vec::new(),
    }
}

#[cfg(windows)]
fn raw_listeners() -> Vec<(String, u16, u32)> {
    use std::net::{Ipv4Addr, Ipv6Addr};
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
    };
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    const ERROR_INSUFFICIENT_BUFFER: u32 = 122;

    fn table(family: u32) -> Vec<u8> {
        let mut size: u32 = 0;
        for _ in 0..4 {
            let mut buffer = vec![0u8; size as usize];
            let status = unsafe {
                GetExtendedTcpTable(buffer.as_mut_ptr().cast(), &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0)
            };
            if status == 0 {
                return buffer;
            }
            if status != ERROR_INSUFFICIENT_BUFFER {
                break;
            }
        }
        Vec::new()
    }

    fn rows<T: Copy>(buffer: &[u8]) -> Vec<T> {
        let Some(count) = buffer.get(..4).map(|b| u32::from_ne_bytes([b[0], b[1], b[2], b[3]]) as usize) else {
            return Vec::new();
        };
        let width = std::mem::size_of::<T>();
        let start = std::mem::align_of::<T>().max(4);
        (0..count)
            .map_while(|i| buffer.get(start + i * width..start + (i + 1) * width))
            .map(|bytes| unsafe { std::ptr::read_unaligned(bytes.as_ptr().cast::<T>()) })
            .collect()
    }

    let port = |raw: u32| u16::from_be(raw as u16);
    let mut out = Vec::new();
    for row in rows::<MIB_TCPROW_OWNER_PID>(&table(AF_INET)) {
        let address = Ipv4Addr::from(u32::from_be(row.dwLocalAddr));
        out.push((address.to_string(), port(row.dwLocalPort), row.dwOwningPid));
    }
    for row in rows::<MIB_TCP6ROW_OWNER_PID>(&table(AF_INET6)) {
        out.push((Ipv6Addr::from(row.ucLocalAddr).to_string(), port(row.dwLocalPort), row.dwOwningPid));
    }
    out
}

pub fn owning_pane(pid: u32, parents: &HashMap<u32, u32>, pane_roots: &HashMap<u32, PaneId>) -> Option<PaneId> {
    let mut current = pid;
    for _ in 0..64 {
        if let Some(pane) = pane_roots.get(&current) {
            return Some(*pane);
        }
        match parents.get(&current) {
            Some(parent) if *parent != current && *parent != 0 => current = *parent,
            _ => return None,
        }
    }
    None
}

fn is_claude(process: &sysinfo::Process) -> bool {
    let name = process.name().to_string_lossy().to_ascii_lowercase();
    match name.trim_end_matches(".exe") {
        "claude" => true,
        "node" => process.cmd().iter().any(|arg| arg.to_string_lossy().contains("claude-code")),
        _ => false,
    }
}

const KEPT_OPTIONS: [&str; 6] = ["-n", "--name", "--model", "--permission-mode", "--add-dir", "--effort"];

pub fn kept_options(args: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.split_once('=') {
            Some((option, _)) if KEPT_OPTIONS.contains(&option) => kept.push(arg.clone()),
            None if KEPT_OPTIONS.contains(&arg.as_str()) => {
                if let Some(value) = args.next() {
                    kept.extend([arg.clone(), value.clone()]);
                }
            }
            _ => {}
        }
    }
    kept
}

pub struct ClaudeProcess {
    pub pid: u32,
    pub skip_permissions: bool,
    pub extra_args: Vec<String>,
    started: u64,
}

pub fn running_claude(system: &mut sysinfo::System, pane_roots: &HashMap<u32, PaneId>) -> HashMap<PaneId, ClaudeProcess> {
    let kind = sysinfo::ProcessRefreshKind::nothing().with_cmd(sysinfo::UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(sysinfo::ProcessesToUpdate::All, true, kind);
    let parents: HashMap<u32, u32> =
        system.processes().iter().filter_map(|(pid, p)| Some((pid.as_u32(), p.parent()?.as_u32()))).collect();
    let mut found: HashMap<PaneId, ClaudeProcess> = HashMap::new();
    for (pid, process) in system.processes().iter().filter(|(_, process)| is_claude(process)) {
        let Some(pane) = owning_pane(pid.as_u32(), &parents, pane_roots) else { continue };
        let started = process.start_time();
        if found.get(&pane).is_some_and(|first| first.started <= started) {
            continue;
        }
        let args: Vec<String> = process.cmd().iter().skip(1).map(|arg| arg.to_string_lossy().into_owned()).collect();
        let skip_permissions = args.iter().any(|arg| arg == "--dangerously-skip-permissions");
        found.insert(pane, ClaudeProcess { pid: pid.as_u32(), skip_permissions, extra_args: kept_options(&args), started });
    }
    found
}

pub fn listening(pane_roots: &HashMap<u32, PaneId>) -> Vec<ListeningPort> {
    let raw = raw_listeners();
    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    let mut parents = HashMap::new();
    let mut names = HashMap::new();
    for (pid, process) in system.processes() {
        names.insert(pid.as_u32(), process.name().to_string_lossy().into_owned());
        if let Some(parent) = process.parent() {
            parents.insert(pid.as_u32(), parent.as_u32());
        }
    }

    let mut ports: Vec<ListeningPort> = Vec::new();
    for (address, port, pid) in raw {
        if ports.iter().any(|p| p.port == port && p.pid == pid) {
            continue;
        }
        ports.push(ListeningPort {
            port,
            address,
            pid,
            process: names.get(&pid).cloned().unwrap_or_default(),
            pane: owning_pane(pid, &parents, pane_roots),
        });
    }
    ports.sort_by_key(|p| (p.pane.is_none(), p.port));
    ports
}

pub fn valid_host(host: &str) -> bool {
    host.trim() == host && nmux_core::validate::ssh_target(host).is_ok()
}

#[derive(Serialize, Deserialize)]
struct SavedForward {
    host: String,
    remote_port: u16,
    local_port: u16,
    #[serde(default)]
    on: bool,
}

struct Entry {
    info: Forward,
    child: Option<Child>,
    run: u64,
    wanted: bool,
    proven: bool,
    tries: usize,
}

impl Entry {
    fn new(id: u32, host: String, remote_port: u16, local_port: u16, wanted: bool) -> Entry {
        let info = Forward { id, host, remote_port, local_port, state: ForwardState::Stopped, error: None };
        Entry { info, child: None, run: 0, wanted, proven: wanted, tries: 0 }
    }

    fn end(&mut self) {
        self.run = 0;
        self.info.state = ForwardState::Stopped;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

type Notify = Arc<dyn Fn(Vec<Forward>) + Send + Sync>;

#[derive(Clone)]
pub struct Forwards {
    inner: Arc<Mutex<Table>>,
    file: std::path::PathBuf,
    notify: Notify,
}

#[derive(Default)]
struct Table {
    entries: Vec<Entry>,
    next_id: u32,
    next_run: u64,
}

impl Table {
    fn list(&self) -> Vec<Forward> {
        self.entries.iter().map(|e| e.info.clone()).collect()
    }

    fn running(&mut self, id: u32, run: u64) -> Option<&mut Entry> {
        self.entries.iter_mut().find(|e| e.info.id == id && e.run == run)
    }
}

const LOOK_EVERY: Duration = Duration::from_millis(500);
const LOOKS: u32 = 40;
const RETRY_SECS: [u64; 6] = [2, 4, 8, 15, 30, 60];
const NEEDS_A_PERSON: [&str; 5] =
    ["Permission denied", "Host key verification failed", "Address already in use", "cannot listen to port", "Could not request local forwarding"];

impl Forwards {
    pub fn load(data_dir: &Path, notify: Notify) -> Forwards {
        let file = data_dir.join("forwards.json");
        let saved: Vec<SavedForward> =
            std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut table = Table::default();
        for s in saved {
            if !valid_host(&s.host) {
                continue;
            }
            table.next_id += 1;
            table.entries.push(Entry::new(table.next_id, s.host, s.remote_port, s.local_port, s.on));
        }
        let wanted: Vec<u32> = table.entries.iter().filter(|e| e.wanted).map(|e| e.info.id).collect();
        let forwards = Forwards { inner: Arc::new(Mutex::new(table)), file, notify };
        for id in wanted {
            let _ = forwards.connect(id);
        }
        forwards
    }

    fn table(&self) -> std::sync::MutexGuard<'_, Table> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn list(&self) -> Vec<Forward> {
        self.table().list()
    }

    fn save(&self, table: &Table) {
        let saved: Vec<SavedForward> = table
            .entries
            .iter()
            .map(|e| SavedForward { host: e.info.host.clone(), remote_port: e.info.remote_port, local_port: e.info.local_port, on: e.wanted })
            .collect();
        if let Ok(body) = serde_json::to_vec_pretty(&saved) {
            let _ = std::fs::write(&self.file, body);
        }
    }

    pub fn start(&self, host: &str, remote_port: u16, local_port: Option<u16>) -> Result<Vec<Forward>, String> {
        if !valid_host(host) {
            return Err("that does not look like an SSH host".into());
        }
        if remote_port == 0 {
            return Err("port 0 cannot be forwarded".into());
        }
        let local_port = local_port.filter(|p| *p != 0).unwrap_or(remote_port);

        let id = {
            let mut table = self.table();
            let index = match table.entries.iter().position(|e| e.info.host == host && e.info.remote_port == remote_port && e.info.local_port == local_port) {
                Some(i) => i,
                None => {
                    table.next_id += 1;
                    let id = table.next_id;
                    table.entries.push(Entry::new(id, host.to_string(), remote_port, local_port, false));
                    table.entries.len() - 1
                }
            };
            let entry = &mut table.entries[index];
            if entry.wanted {
                return Ok(table.list());
            }
            entry.wanted = true;
            entry.proven = false;
            entry.tries = 0;
            entry.info.error = None;
            entry.info.id
        };
        self.connect(id)?;
        Ok(self.list())
    }

    fn connect(&self, id: u32) -> Result<(), String> {
        let mut table = self.table();
        let Some(index) = table.entries.iter().position(|e| e.info.id == id && e.wanted && e.child.is_none()) else { return Ok(()) };
        let info = table.entries[index].info.clone();
        let mut command = Command::new("ssh");
        quiet(&mut command)
            .arg("-N")
            .args(["-o", "ControlPath=none"])
            .args(["-o", "ExitOnForwardFailure=yes"])
            .args(["-o", "BatchMode=yes"])
            .args(["-o", "ConnectTimeout=15"])
            .args(["-o", "ServerAliveInterval=15"])
            .args(["-o", "ServerAliveCountMax=3"])
            .arg("-L")
            .arg(format!("127.0.0.1:{}:localhost:{}", info.local_port, info.remote_port))
            .arg("--")
            .arg(&info.host)
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                let why = format!("could not run ssh: {e}");
                let entry = &mut table.entries[index];
                entry.wanted = false;
                entry.info.state = ForwardState::Failed;
                entry.info.error = Some(why.clone());
                self.save(&table);
                return Err(why);
            }
        };
        let stderr = child.stderr.take();
        let pid = child.id();

        table.next_run += 1;
        let run = table.next_run;
        let entry = &mut table.entries[index];
        entry.child = Some(child);
        entry.run = run;
        entry.info.state = ForwardState::Starting;
        self.save(&table);
        drop(table);

        let me = self.clone();
        std::thread::spawn(move || {
            for look in 1..=LOOKS {
                std::thread::sleep(LOOK_EVERY);
                let open = look == LOOKS || raw_listeners().iter().any(|l| l.1 == info.local_port && l.2 == pid);
                let mut table = me.table();
                let Some(entry) = table.running(id, run).filter(|e| e.child.is_some()) else { return };
                if open {
                    entry.info.state = ForwardState::Active;
                    entry.info.error = None;
                    entry.proven = true;
                    entry.tries = 0;
                    let list = table.list();
                    drop(table);
                    (me.notify)(list);
                    return;
                }
            }
        });

        let me = self.clone();
        std::thread::spawn(move || {
            let mut last = String::new();
            if let Some(stderr) = stderr {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if !line.trim().is_empty() {
                        last = line.trim().chars().take(300).collect();
                    }
                }
            }
            let mut table = me.table();
            let Some(entry) = table.running(id, run) else { return };
            if let Some(mut child) = entry.child.take() {
                let _ = child.wait();
            }
            let mut wait = None;
            if entry.wanted {
                let again = entry.proven && !NEEDS_A_PERSON.iter().any(|text| last.contains(text));
                entry.info.error = Some(if last.is_empty() { "ssh exited".to_string() } else { last });
                if again {
                    entry.info.state = ForwardState::Starting;
                    wait = Some(RETRY_SECS[entry.tries.min(RETRY_SECS.len() - 1)]);
                    entry.tries += 1;
                } else {
                    entry.info.state = ForwardState::Failed;
                    entry.wanted = false;
                    me.save(&table);
                }
            }
            let list = table.list();
            drop(table);
            (me.notify)(list);
            if let Some(wait) = wait {
                std::thread::sleep(Duration::from_secs(wait));
                if me.table().running(id, run).is_some() && me.connect(id).is_err() {
                    (me.notify)(me.list());
                }
            }
        });

        Ok(())
    }

    pub fn stop(&self, id: u32) -> Result<Vec<Forward>, String> {
        let mut table = self.table();
        let entry = table.entries.iter_mut().find(|e| e.info.id == id).ok_or("no such forward")?;
        entry.wanted = false;
        entry.info.error = None;
        entry.end();
        self.save(&table);
        Ok(table.list())
    }

    pub fn remove(&self, id: u32) -> Result<Vec<Forward>, String> {
        self.stop(id)?;
        let mut table = self.table();
        table.entries.retain(|e| e.info.id != id);
        self.save(&table);
        Ok(table.list())
    }

    pub fn stop_all(&self) {
        let mut table = self.table();
        for entry in table.entries.iter_mut() {
            entry.end();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_claude_keeps_its_options_but_not_its_prompt() {
        let args = |text: &str| text.split(' ').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(kept_options(&args("-n Deployment --resume abc --model=opus fix-it -c --add-dir ../x --effort")), args("-n Deployment --model=opus --add-dir ../x"));
    }

    #[cfg(windows)]
    #[test]
    fn a_port_this_process_listens_on_is_found() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let found = raw_listeners();
        assert!(found.contains(&("127.0.0.1".to_string(), port, std::process::id())), "{port} not in {found:?}");
        drop(listener);
        assert!(!raw_listeners().iter().any(|l| l.1 == port && l.2 == std::process::id()));
    }

    #[test]
    fn ss_listeners_are_parsed() {
        let text = "LISTEN 0 511 127.0.0.1:5173 0.0.0.0:* users:((\"node\",pid=1234,fd=20))\nLISTEN 0 128 [::]:22 [::]:*\n";
        assert_eq!(parse_ss(text), vec![("127.0.0.1".to_string(), 5173, 1234), ("::".to_string(), 22, 0)]);
    }

    #[test]
    fn a_port_belongs_to_the_pane_above_it() {
        let parents = HashMap::from([(30, 20), (20, 10), (99, 1), (5, 5)]);
        let roots = HashMap::from([(10, PaneId(7))]);
        assert_eq!(owning_pane(30, &parents, &roots), Some(PaneId(7)));
        assert_eq!(owning_pane(10, &parents, &roots), Some(PaneId(7)));
        assert_eq!(owning_pane(99, &parents, &roots), None);
        assert_eq!(owning_pane(5, &parents, &roots), None, "a self-parent loop ends");
    }

    #[test]
    fn hosts_cannot_smuggle_options() {
        for ok in ["devbox", "me@server.example.com", "10.0.0.5", "[::1]", "build_1"] {
            assert!(valid_host(ok), "{ok}");
        }
        for bad in ["", "-oProxyCommand=calc", "a b", "host;rm", "$(x)", "a\"b"] {
            assert!(!valid_host(bad), "{bad}");
        }
    }

    #[test]
    fn a_forward_that_cannot_connect_fails_with_a_reason_and_is_remembered() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let notify: Notify = Arc::new(move |list| {
            let _ = tx.send(list);
        });
        let forwards = Forwards::load(dir.path(), notify.clone());
        assert!(forwards.start("-bad", 80, None).is_err());
        assert!(forwards.start("ok", 0, None).is_err());

        let list = match forwards.start("nmux-test.invalid", 8080, None) {
            Ok(list) => list,
            Err(_) => return,
        };
        assert_eq!((list[0].local_port, list[0].state), (8080, ForwardState::Starting));
        let failed = loop {
            let list = rx.recv_timeout(Duration::from_secs(30)).expect("ssh should give up");
            if list[0].state == ForwardState::Failed {
                break list;
            }
        };
        assert!(failed[0].error.as_deref().is_some_and(|e| !e.is_empty()));

        let again = Forwards::load(dir.path(), notify);
        let list = again.list();
        assert_eq!((list.len(), list[0].state, list[0].host.as_str()), (1, ForwardState::Stopped, "nmux-test.invalid"));
        assert!(again.remove(list[0].id).unwrap().is_empty());
    }

    #[test]
    fn a_forward_that_was_on_comes_back_and_keeps_trying() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("forwards.json");
        std::fs::write(&file, r#"[{"host":"nmux-test.invalid","remote_port":8080,"local_port":8080,"on":true}]"#).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let forwards = Forwards::load(
            dir.path(),
            Arc::new(move |list| {
                let _ = tx.send(list);
            }),
        );
        let first = forwards.list();
        if first[0].state != ForwardState::Starting {
            return;
        }
        let lost = rx.recv_timeout(Duration::from_secs(30)).expect("ssh should give up");
        assert_eq!(lost[0].state, ForwardState::Starting, "it is tried again, not left failed");
        assert!(lost[0].error.as_deref().is_some_and(|e| !e.is_empty()));
        assert!(std::fs::read_to_string(&file).unwrap().contains("\"on\": true"));
        forwards.stop(first[0].id).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("\"on\": false"));
    }
}
