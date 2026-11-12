use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::SystemTime;

use nmux_core::{Dir, PaneId, PaneKind, PaneSpec, PaneStatus, Side, Tab};
use nmux_proto::{Op, Reply};

use super::{fail, key_bytes, pane_state, Cli};

type Command = (&'static str, &'static str, &'static str, &'static str);

const JOIN: &str = "[-bdhv] [-s pane] [-t pane]";

const COMMANDS: &[Command] = &[
    ("attach-session", "attach", "t:", "[-t target]"),
    ("break-pane", "breakp", "dF:n:Ps:", "[-dP] [-n name] [-s pane]"),
    ("capture-pane", "capturep", "b:eE:JpS:t:", "[-p] [-b buffer] [-S start] [-E end] [-t pane]"),
    ("delete-buffer", "deleteb", "b:", "[-b buffer]"),
    ("display-message", "display", "F:pt:", "[-p] [-t pane] [message]"),
    ("has-session", "has", "t:", "[-t session]"),
    ("join-pane", "joinp", "bdhs:t:v", JOIN),
    ("kill-pane", "killp", "at:", "[-a] [-t pane]"),
    ("kill-server", "", "", ""),
    ("kill-session", "", "at:", "[-a] [-t session]"),
    ("kill-window", "killw", "at:", "[-a] [-t window]"),
    ("list-buffers", "lsb", "", ""),
    ("list-commands", "lscm", "", ""),
    ("list-panes", "lsp", "aF:st:", "[-as] [-F format] [-t window]"),
    ("list-sessions", "", "F:", "[-F format]"),
    ("list-windows", "lsw", "aF:t:", "[-a] [-F format] [-t session]"),
    ("load-buffer", "loadb", "b:", "[-b buffer] path"),
    ("move-pane", "movep", "bdhs:t:v", JOIN),
    ("new-session", "new", "c:dF:n:Ps:x:y:", "[-dP] [-s name] [-n tab-name] [-c folder] [command]"),
    ("new-window", "neww", "c:dF:n:Pt:", "[-dP] [-n name] [-c folder] [-t session] [command]"),
    ("next-window", "next", "t:", "[-t session]"),
    ("paste-buffer", "pasteb", "b:drt:", "[-dr] [-b buffer] [-t pane]"),
    ("previous-window", "prev", "t:", "[-t session]"),
    ("rename-session", "rename", "t:", "[-t session] name"),
    ("rename-window", "renamew", "t:", "[-t window] name"),
    ("resize-pane", "resizep", "DLRt:UZ", "[-DLRUZ] [-t pane] [cells]"),
    ("respawn-pane", "respawnp", "kt:", "[-k] [-t pane]"),
    ("respawn-window", "respawnw", "kt:", "[-k] [-t window]"),
    ("rotate-window", "rotatew", "Dt:U", "[-DU] [-t window]"),
    ("save-buffer", "saveb", "ab:", "[-a] [-b buffer] path"),
    ("select-pane", "selectp", "DLP:Rt:T:U", "[-DLRU] [-T title] [-t pane]"),
    ("select-window", "selectw", "npt:", "[-np] [-t window]"),
    ("send-keys", "", "HlN:t:", "[-Hl] [-N count] [-t pane] key ..."),
    ("set-buffer", "setb", "ab:", "[-a] [-b buffer] text"),
    ("show-buffer", "showb", "b:", "[-b buffer]"),
    ("split-window", "splitw", "bc:dF:hl:p:Pt:v", "[-bdhvP] [-l size] [-c folder] [-t pane] [command]"),
    ("swap-pane", "swapp", "Ds:t:U", "[-DU] [-s pane] [-t pane]"),
    ("switch-client", "switchc", "npt:", "[-np] [-t target]"),
];

const ABSENT: &[(&str, &str, bool)] = &[
    ("bind-key", "bind", true),
    ("choose-buffer", "", false),
    ("choose-client", "", false),
    ("choose-tree", "", false),
    ("clear-history", "clearhist", true),
    ("clear-prompt-history", "clearphist", false),
    ("clock-mode", "", false),
    ("command-prompt", "", false),
    ("confirm-before", "confirm", false),
    ("copy-mode", "", false),
    ("customize-mode", "", false),
    ("detach-client", "detach", false),
    ("display-menu", "menu", false),
    ("display-panes", "displayp", false),
    ("display-popup", "popup", false),
    ("find-window", "findw", false),
    ("if-shell", "if", false),
    ("last-pane", "lastp", false),
    ("last-window", "last", false),
    ("link-window", "linkw", false),
    ("list-clients", "lsc", false),
    ("list-keys", "lsk", false),
    ("lock-client", "lockc", false),
    ("lock-server", "lock", false),
    ("lock-session", "locks", false),
    ("move-window", "movew", false),
    ("next-layout", "nextl", true),
    ("pipe-pane", "pipep", false),
    ("previous-layout", "prevl", true),
    ("refresh-client", "refresh", true),
    ("resize-window", "resizew", true),
    ("run-shell", "", false),
    ("select-layout", "selectl", true),
    ("send-prefix", "", false),
    ("server-access", "", false),
    ("set-environment", "setenv", true),
    ("set-hook", "", true),
    ("set-option", "set", true),
    ("set-window-option", "setw", true),
    ("show-environment", "showenv", false),
    ("show-hooks", "", false),
    ("show-messages", "showmsgs", false),
    ("show-options", "show", false),
    ("show-prompt-history", "showphist", false),
    ("show-window-options", "showw", false),
    ("source-file", "source", true),
    ("start-server", "start", true),
    ("suspend-client", "suspendc", false),
    ("swap-window", "swapw", false),
    ("unbind-key", "unbind", true),
    ("unlink-window", "unlinkw", false),
    ("wait-for", "", false),
];

fn find(name: &str, as_tmux: bool) -> Result<Option<&'static Command>, String> {
    if let Some(command) = COMMANDS.iter().find(|c| c.0 == name || (c.1 == name && !name.is_empty())) {
        return Ok(Some(command));
    }
    if let Some((_, _, harmless)) = ABSENT.iter().find(|(long, short, _)| *long == name || *short == name) {
        return if as_tmux && *harmless { Ok(None) } else { fail(format!("tmux's {name} has nothing like it in Nmux")) };
    }
    match COMMANDS.iter().filter(|c| c.0.starts_with(name)).collect::<Vec<_>>().as_slice() {
        [command] => Ok(Some(command)),
        [] => fail(format!("unknown command \"{name}\"; run: nc --help")),
        many => fail(format!("\"{name}\" could be: {}", many.iter().map(|c| c.0).collect::<Vec<_>>().join(", "))),
    }
}

struct Opts {
    flags: Vec<(char, Option<String>)>,
    words: Vec<String>,
}

impl Opts {
    fn parse(template: &str, raw: &[String]) -> Result<Opts, String> {
        let mut flags = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            let arg = &raw[i];
            if arg == "--" {
                i += 1;
                break;
            }
            if !arg.starts_with('-') || arg.len() == 1 {
                break;
            }
            for (at, flag) in arg.char_indices().skip(1) {
                let Some(found) = template.find(flag).filter(|_| flag != ':') else { return fail(format!("unknown flag -{flag}")) };
                if !template[found + flag.len_utf8()..].starts_with(':') {
                    flags.push((flag, None));
                    continue;
                }
                let inline = &arg[at + flag.len_utf8()..];
                let value = if inline.is_empty() {
                    i += 1;
                    raw.get(i).cloned().ok_or(format!("-{flag} needs a value"))?
                } else {
                    inline.to_string()
                };
                flags.push((flag, Some(value)));
                break;
            }
            i += 1;
        }
        Ok(Opts { flags, words: raw[i.min(raw.len())..].to_vec() })
    }

    fn has(&self, flag: char) -> bool {
        self.flags.iter().any(|(f, _)| *f == flag)
    }

    fn get(&self, flag: char) -> Option<&str> {
        self.flags.iter().rev().find(|(f, _)| *f == flag).and_then(|(_, value)| value.as_deref())
    }

    fn side(&self) -> Option<Side> {
        [('L', Side::Left), ('R', Side::Right), ('U', Side::Up), ('D', Side::Down)].into_iter().find(|(flag, _)| self.has(*flag)).map(|(_, side)| side)
    }
}

#[derive(Clone, Copy)]
struct Place {
    ws: usize,
    tab: usize,
    pane: PaneId,
}

#[derive(Clone, Copy, PartialEq)]
enum Level {
    Session,
    Window,
    Pane,
}

fn closing(body: &str) -> usize {
    let mut depth = 0;
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' if depth == 0 => return i,
            '}' => depth -= 1,
            _ => {}
        }
    }
    body.len()
}

fn choices(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (i, c) in body.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            ',' if depth == 0 && parts.len() < 2 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

fn percent(text: &str) -> Result<f32, String> {
    text.trim_end_matches('%').parse::<f32>().map(|p| p / 100.0).map_err(|_| format!("bad size \"{text}\""))
}

fn saved(dir: &PathBuf) -> Vec<(SystemTime, PathBuf)> {
    let mut all: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    all.sort_by(|a, b| b.cmp(a));
    all
}

impl Cli {
    fn place(&self, pane: PaneId) -> Result<Place, String> {
        let (ws, tab) = self.state.locate(pane).ok_or("that pane was closed")?;
        Ok(Place { ws, tab, pane })
    }

    fn in_tab(&self, ws: usize, tab: usize) -> Place {
        Place { ws, tab, pane: self.state.workspaces[ws].tabs[tab].focused }
    }

    fn in_workspace(&self, ws: usize) -> Place {
        let w = &self.state.workspaces[ws];
        self.in_tab(ws, w.tabs.iter().position(|t| t.id == w.active_tab).unwrap_or(0))
    }

    fn in_front(&self) -> Option<PaneId> {
        self.state.workspace(self.state.active_workspace?)?.focused_pane()
    }

    fn here(&self) -> Result<Place, String> {
        self.place(self.me().or(self.in_front()).ok_or("there are no panes")?)
    }

    fn session(&self, text: &str) -> Result<usize, String> {
        let pane = match text.parse::<u32>() {
            Ok(number) => self.state.resolve(&format!("n{number}")),
            Err(_) => self.state.resolve(text),
        };
        Ok(self.place(pane.map_err(|_| format!("can't find session {text}"))?)?.ws)
    }

    fn target(&self, text: Option<&str>, level: Level) -> Result<Place, String> {
        let Some(text) = text.map(str::trim).filter(|t| !t.is_empty()) else { return self.here() };
        let missing = || format!("can't find {text}");
        let id = |rest: &str| rest.parse::<u32>().map_err(|_| missing());
        if let Some(rest) = text.strip_prefix('%') {
            return self.place(PaneId(id(rest)?)).map_err(|_| missing());
        }
        if let Some(rest) = text.strip_prefix('@') {
            let wanted = id(rest)?;
            let found = self.state.workspaces.iter().enumerate().find_map(|(ws, w)| Some(self.in_tab(ws, w.tabs.iter().position(|t| t.id.0 == wanted)?)));
            return found.ok_or_else(missing);
        }
        if let Some(rest) = text.strip_prefix('$') {
            let wanted = id(rest)?;
            return self.state.workspaces.iter().position(|w| w.id.0 == wanted).map(|ws| self.in_workspace(ws)).ok_or_else(missing);
        }
        let numbers = text.chars().all(|c| c.is_ascii_digit() || c == '.');
        if !numbers && !text.contains(':') {
            if let Ok(pane) = self.state.resolve(text) {
                return self.place(pane);
            }
        }
        let (session, rest) = match text.split_once(':') {
            Some(parts) => parts,
            None if level == Level::Session => (text, ""),
            None => ("", text),
        };
        let mut at = if session.is_empty() { self.here()? } else { self.in_workspace(self.session(session)?) };
        let (window, pane) = match rest.rsplit_once('.') {
            Some(parts) => parts,
            None if level == Level::Pane && !text.contains(':') => ("", rest),
            None => (rest, ""),
        };
        if !window.is_empty() {
            let tabs = &self.state.workspaces[at.ws].tabs;
            let tab = match window.parse::<usize>() {
                Ok(index) => (index < tabs.len()).then_some(index),
                Err(_) => tabs.iter().position(|t| self.tab_name(t) == window),
            };
            at = self.in_tab(at.ws, tab.ok_or(format!("can't find window {window}"))?);
        }
        if !pane.is_empty() {
            let panes = self.panes_of(&at);
            at.pane = pane.parse::<usize>().ok().and_then(|index| panes.get(index).copied()).ok_or(format!("can't find pane {pane}"))?;
        }
        Ok(at)
    }

    fn tab_name(&self, tab: &Tab) -> String {
        tab.name.clone().or_else(|| self.state.panes.get(&tab.focused).map(|p| p.name.clone())).unwrap_or_default()
    }

    fn panes_of(&self, at: &Place) -> Vec<PaneId> {
        self.state.workspaces[at.ws].tabs[at.tab].layout.panes()
    }

    fn tabs_of(&self, ws: usize) -> Vec<Place> {
        (0..self.state.workspaces[ws].tabs.len()).map(|tab| self.in_tab(ws, tab)).collect()
    }

    fn every_tab(&self) -> Vec<Place> {
        (0..self.state.workspaces.len()).flat_map(|ws| self.tabs_of(ws)).collect()
    }

    fn each_pane(&self, tabs: &[Place]) -> Vec<Place> {
        tabs.iter().flat_map(|at| self.panes_of(at).into_iter().map(|pane| Place { pane, ..*at })).collect()
    }

    fn var(&self, name: &str, at: &Place) -> String {
        let ws = &self.state.workspaces[at.ws];
        let tab = &ws.tabs[at.tab];
        let Some(pane) = self.state.panes.get(&at.pane) else { return String::new() };
        let flag = |on: bool| if on { "1" } else { "0" }.to_string();
        let front = ws.active_tab == tab.id;
        match name {
            "session_name" => ws.name.clone(),
            "session_id" => format!("${}", ws.id.0),
            "session_number" => ws.number.to_string(),
            "session_path" => ws.cwd.clone(),
            "session_windows" => ws.tabs.len().to_string(),
            "session_attached" => flag(self.state.active_workspace == Some(ws.id)),
            "window_index" => at.tab.to_string(),
            "window_id" => format!("@{}", tab.id.0),
            "window_name" => self.tab_name(tab),
            "window_panes" => tab.layout.panes().len().to_string(),
            "window_active" => flag(front),
            "window_zoomed_flag" => flag(tab.zoomed),
            "window_flags" => format!("{}{}", if front { "*" } else { "" }, if tab.zoomed { "Z" } else { "" }),
            "pane_index" => tab.layout.panes().iter().position(|p| *p == at.pane).unwrap_or(0).to_string(),
            "pane_id" => format!("%{}", at.pane.0),
            "pane_address" => self.state.address(at.pane).unwrap_or_default(),
            "pane_active" => flag(tab.focused == at.pane),
            "pane_title" => pane.name.clone(),
            "pane_current_path" => pane.cwd.clone(),
            "pane_current_command" => format!("{:?}", pane.shown_kind()).to_lowercase(),
            "pane_state" => pane_state(pane),
            "pane_dead" => flag(pane.status == PaneStatus::Exited),
            _ => String::new(),
        }
    }

    fn expand(&self, format: &str, at: &Place) -> String {
        let mut out = String::new();
        let mut rest = format;
        while let Some(i) = rest.find('#') {
            out.push_str(&rest[..i]);
            rest = &rest[i + 1..];
            if let Some(body) = rest.strip_prefix('{') {
                let end = closing(body);
                let inner = &body[..end];
                out.push_str(&match inner.strip_prefix('?').map(choices).as_deref() {
                    Some([test, yes, no]) => self.expand(if matches!(self.var(test, at).as_str(), "" | "0") { no } else { yes }, at),
                    Some(_) => String::new(),
                    None => self.var(inner, at),
                });
                rest = &body[(end + 1).min(body.len())..];
                continue;
            }
            let mut after = rest.chars();
            let name = match after.next() {
                Some('S') => "session_name",
                Some('I') => "window_index",
                Some('W') => "window_name",
                Some('F') => "window_flags",
                Some('P') => "pane_index",
                Some('T') => "pane_title",
                Some('D') => "pane_id",
                Some('#') => {
                    out.push('#');
                    rest = after.as_str();
                    continue;
                }
                _ => {
                    out.push('#');
                    continue;
                }
            };
            out.push_str(&self.var(name, at));
            rest = after.as_str();
        }
        out + rest
    }

    fn show(&self, places: &[Place], o: &Opts, format: &str) {
        for at in places {
            println!("{}", self.expand(o.get('F').unwrap_or(format), at));
        }
    }

    async fn all(&self, ops: Vec<Op>) -> Result<(), String> {
        for op in ops {
            self.call(op).await?;
        }
        Ok(())
    }

    async fn size(&self, pane: PaneId) -> Result<(f32, f32), String> {
        let Reply::Attached { snapshot } = self.call(Op::Attach { pane }).await? else { return fail("unexpected reply") };
        Ok((snapshot.cols as f32, snapshot.rows as f32))
    }

    async fn opened(&mut self, pane: PaneId, back: [Option<PaneId>; 3], o: &Opts) -> Result<(), String> {
        for back in back.into_iter().flatten().filter(|_| o.has('d')) {
            self.call(Op::FocusPane { pane: back }).await?;
        }
        self.refresh().await?;
        if o.has('P') {
            println!("{}", self.expand(o.get('F').unwrap_or("#{session_name}:#{window_index}.#{pane_index}"), &self.place(pane)?));
        }
        self.type_when_ready(pane, PaneKind::Shell, o.words.join(" ")).await
    }

    fn buffer(&self, name: Option<&str>, new: bool) -> Result<PathBuf, String> {
        let dir = self.data_dir.join("buffers");
        match name {
            Some(name) if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') => {
                fail("a buffer name is made of letters, digits, - and _")
            }
            Some(name) => Ok(dir.join(name)),
            None if new => Ok((0..).map(|n| dir.join(format!("buffer{n}"))).find(|path| !path.exists()).unwrap_or(dir)),
            None => saved(&dir).into_iter().next().map(|(_, path)| path).ok_or_else(|| "there are no buffers".to_string()),
        }
    }

    fn keep(&self, name: Option<&str>, text: &str, append: bool) -> Result<(), String> {
        let path = self.buffer(name, !append)?;
        let before = if append { std::fs::read_to_string(&path).unwrap_or_default() } else { String::new() };
        std::fs::create_dir_all(self.data_dir.join("buffers")).and_then(|_| std::fs::write(&path, before + text)).map_err(|e| e.to_string())
    }

    fn kept(&self, name: Option<&str>) -> Result<(PathBuf, String), String> {
        let path = self.buffer(name, false)?;
        let text = std::fs::read_to_string(&path).map_err(|_| "there is no such buffer")?;
        Ok((path, text))
    }

    async fn tmux(&mut self, name: &str, raw: &[String]) -> Result<(), String> {
        let name = match name {
            "ls" => "list-sessions",
            "send" => "send-keys",
            name => name,
        };
        let Some(&(command, _, template, usage)) = find(name, self.as_tmux)? else { return Ok(()) };
        let o = Opts::parse(template, raw).map_err(|why| format!("{why}; usage: nc {command} {usage}"))?;
        let t = o.get('t');
        let word = o.words.first().map(String::as_str);
        let named = || word.map(str::to_string).ok_or(format!("usage: nc {command} {usage}"));
        let front = self.in_front();
        let mut spec = PaneSpec::new(PaneKind::Shell);
        spec.cwd = o.get('c').filter(|_| template.contains("c:")).map(str::to_string);
        match command {
            "attach-session" | "switch-client" if o.has('n') || o.has('p') => {
                let now = self.place(front.ok_or("there are no panes")?)?.ws;
                let count = self.state.workspaces.len();
                let next = (now + if o.has('n') { 1 } else { count - 1 }) % count;
                self.call(Op::SelectWorkspace { workspace: self.state.workspaces[next].id }).await?;
            }
            "attach-session" | "switch-client" => {
                let pane = self.target(t.or(word), Level::Session)?.pane;
                self.call(Op::FocusPane { pane }).await?;
            }
            "has-session" => {
                self.target(t, Level::Session)?;
            }
            "new-session" => {
                let cwd = match spec.cwd.take() {
                    Some(cwd) => cwd,
                    None => std::env::current_dir().map_err(|e| e.to_string())?.to_string_lossy().into_owned(),
                };
                let created = self.open(Op::CreateWorkspace { name: o.get('s').map(str::to_string), cwd, pane: spec, machine: None }).await?;
                if let Some(name) = o.get('n') {
                    self.call(Op::RenameTab { tab: created.tab, name: Some(name.to_string()) }).await?;
                }
                self.opened(created.pane, [None, None, front], &o).await?;
            }
            "new-window" => {
                let at = self.target(t, Level::Session)?;
                let workspace = self.state.workspaces[at.ws].id;
                let shown = self.in_workspace(at.ws).pane;
                let created = self.open(Op::CreateTab { workspace, name: o.get('n').map(str::to_string), pane: spec }).await?;
                self.opened(created.pane, [None, Some(shown), front], &o).await?;
            }
            "split-window" => {
                let at = self.target(t, Level::Pane)?;
                let (target, shown) = (at.pane, [Some(self.in_tab(at.ws, at.tab).pane), Some(self.in_workspace(at.ws).pane), front]);
                let wide = o.has('h');
                let share = match (o.get('p'), o.get('l')) {
                    (Some(size), _) => Some(percent(size)?),
                    (_, Some(size)) if size.ends_with('%') => Some(percent(size)?),
                    (_, Some(size)) => {
                        let (cols, rows) = self.size(target).await?;
                        Some(size.parse::<f32>().map_err(|_| format!("bad size \"{size}\""))? / if wide { cols } else { rows })
                    }
                    _ => None,
                };
                let pane = self.open(Op::SplitPane { target, dir: if wide { Dir::Right } else { Dir::Down }, pane: spec }).await?.pane;
                let (near, far) = if wide { (Side::Left, Side::Right) } else { (Side::Up, Side::Down) };
                if o.has('b') {
                    self.call(Op::MovePane { pane, target, side: near }).await?;
                }
                if let Some(share) = share {
                    self.call(Op::ResizeSplit { pane, side: if o.has('b') { far } else { near }, delta: share - 0.5 }).await?;
                }
                self.opened(pane, shown, &o).await?;
            }
            "break-pane" => {
                let pane = self.target(o.get('s'), Level::Pane)?.pane;
                self.call(Op::BreakPane { pane }).await?;
                self.refresh().await?;
                if let Some(name) = o.get('n') {
                    let tab = self.place(pane)?;
                    self.call(Op::RenameTab { tab: self.state.workspaces[tab.ws].tabs[tab.tab].id, name: Some(name.to_string()) }).await?;
                }
                self.opened(pane, [None, None, front], &Opts { words: Vec::new(), ..o }).await?;
            }
            "join-pane" | "move-pane" => {
                let pane = self.target(o.get('s'), Level::Pane)?.pane;
                let target = self.target(t, Level::Pane)?.pane;
                let side = match (o.has('h'), o.has('b')) {
                    (true, true) => Side::Left,
                    (true, false) => Side::Right,
                    (false, true) => Side::Up,
                    (false, false) => Side::Down,
                };
                self.call(Op::MovePane { pane, target, side }).await?;
                if let Some(back) = front.filter(|_| o.has('d')) {
                    self.call(Op::FocusPane { pane: back }).await?;
                }
            }
            "kill-pane" => {
                let at = self.target(t, Level::Pane)?;
                let others = self.panes_of(&at).into_iter().filter(|p| *p != at.pane).collect();
                let gone: Vec<PaneId> = if o.has('a') { others } else { vec![at.pane] };
                self.all(gone.into_iter().map(|pane| Op::ClosePane { pane }).collect()).await?;
            }
            "kill-window" => {
                let at = self.target(t, Level::Window)?;
                let tabs = &self.state.workspaces[at.ws].tabs;
                let gone = tabs.iter().enumerate().filter(|(i, _)| (*i == at.tab) != o.has('a'));
                self.all(gone.map(|(_, tab)| Op::CloseTab { tab: tab.id }).collect()).await?;
            }
            "kill-session" => {
                let at = self.target(t, Level::Session)?;
                let gone = self.state.workspaces.iter().enumerate().filter(|(i, _)| (*i == at.ws) != o.has('a'));
                self.all(gone.map(|(_, w)| Op::CloseWorkspace { workspace: w.id }).collect()).await?;
            }
            "kill-server" => {
                self.call(Op::Shutdown).await?;
            }
            "rename-session" => {
                let workspace = self.state.workspaces[self.target(t, Level::Session)?.ws].id;
                self.call(Op::RenameWorkspace { workspace, name: named()? }).await?;
            }
            "rename-window" => {
                let at = self.target(t, Level::Window)?;
                self.call(Op::RenameTab { tab: self.state.workspaces[at.ws].tabs[at.tab].id, name: Some(named()?) }).await?;
            }
            "select-window" | "next-window" | "previous-window" => {
                let level = if command == "select-window" { Level::Window } else { Level::Session };
                let mut at = self.target(t, level)?;
                let tabs = &self.state.workspaces[at.ws].tabs;
                if command == "next-window" || o.has('n') {
                    at.tab = (at.tab + 1) % tabs.len();
                } else if command == "previous-window" || o.has('p') {
                    at.tab = (at.tab + tabs.len() - 1) % tabs.len();
                }
                self.call(Op::SelectTab { tab: tabs[at.tab].id }).await?;
            }
            "select-pane" => {
                let pane = self.target(t, Level::Pane)?.pane;
                if o.has('P') {
                } else if let Some(title) = o.get('T') {
                    self.call(Op::RenamePane { pane, name: title.to_string() }).await?;
                } else if let Some(side) = o.side() {
                    self.call(Op::FocusSide { pane, side }).await?;
                } else {
                    self.call(Op::FocusPane { pane }).await?;
                }
            }
            "swap-pane" => {
                let at = self.target(t, Level::Pane)?;
                let panes = self.panes_of(&at);
                let index = panes.iter().position(|p| *p == at.pane).unwrap_or(0);
                let other = if o.has('U') {
                    panes[(index + panes.len() - 1) % panes.len()]
                } else if o.has('D') {
                    panes[(index + 1) % panes.len()]
                } else {
                    self.target(o.get('s'), Level::Pane)?.pane
                };
                self.call(Op::SwapPanes { a: other, b: at.pane }).await?;
            }
            "rotate-window" => {
                let panes = self.panes_of(&self.target(t, Level::Window)?);
                let (moved, past): (PaneId, Vec<PaneId>) = match panes.split_last() {
                    Some((last, rest)) if o.has('D') => (*last, rest.iter().rev().copied().collect()),
                    _ => (panes[0], panes[1..].to_vec()),
                };
                self.all(past.into_iter().map(|b| Op::SwapPanes { a: moved, b }).collect()).await?;
            }
            "resize-pane" => {
                let pane = self.target(t, Level::Pane)?.pane;
                let Some(side) = o.side() else {
                    if !o.has('Z') {
                        return fail(format!("usage: nc {command} {usage}"));
                    }
                    self.call(Op::ToggleZoom { pane }).await?;
                    return Ok(());
                };
                let cells = word.map_or(Ok(1.0), |n| n.parse::<f32>().map_err(|_| format!("bad size \"{n}\"")))?;
                let (cols, rows) = self.size(pane).await?;
                let (edge, other, room) = if matches!(side, Side::Left | Side::Right) { (Side::Right, Side::Left, cols) } else { (Side::Down, Side::Up, rows) };
                let delta = cells / (2.0 * room) * if side == edge { 1.0 } else { -1.0 };
                if self.call(Op::ResizeSplit { pane, side: edge, delta }).await? == (Reply::Flag { value: false }) {
                    self.call(Op::ResizeSplit { pane, side: other, delta: -delta }).await?;
                }
            }
            "respawn-pane" | "respawn-window" => {
                let at = self.target(t, if command == "respawn-pane" { Level::Pane } else { Level::Window })?;
                let panes = if command == "respawn-pane" { vec![at.pane] } else { self.panes_of(&at) };
                for pane in panes {
                    if self.pane(pane)?.status == PaneStatus::Running {
                        if !o.has('k') {
                            return fail("it is still running; add -k to end it first");
                        }
                        self.call(Op::StopPane { pane }).await?;
                    }
                    self.call(Op::RestartPane { pane }).await?;
                }
            }
            "send-keys" => {
                let pane = self.target(t, Level::Pane)?.pane;
                let once: String = if o.has('H') {
                    let bytes: Result<Vec<u8>, _> = o.words.iter().map(|w| u8::from_str_radix(w, 16)).collect();
                    String::from_utf8(bytes.map_err(|_| "-H takes bytes in hex, like 1b 5b 41")?).map_err(|_| "those bytes are not text")?
                } else if o.has('l') {
                    o.words.concat()
                } else {
                    o.words.iter().map(|key| key_bytes(key)).collect()
                };
                if once.is_empty() {
                    return fail(format!("usage: nc {command} {usage}"));
                }
                let count = o.get('N').and_then(|n| n.parse::<usize>().ok()).unwrap_or(1);
                self.call(Op::Write { pane, data: once.repeat(count) }).await?;
            }
            "capture-pane" => {
                let pane = self.target(t, Level::Pane)?.pane;
                let (lines, above) = self.rows(pane, true).await?;
                let last = lines.len() as i64 - 1;
                let line = |flag: char, unset: i64, dash: i64| match o.get(flag) {
                    None => Ok(unset),
                    Some("-") => Ok(dash),
                    Some(n) => n.parse::<i64>().map(|n| (above as i64 + n).clamp(0, last)).map_err(|_| format!("bad line \"{n}\"")),
                };
                let (start, end) = (line('S', above as i64, 0)?, line('E', last, last)?);
                let text = if start > end { String::new() } else { lines[start as usize..=end as usize].join("\n") };
                if o.has('p') {
                    println!("{text}");
                } else {
                    self.keep(o.get('b'), &text, false)?;
                }
            }
            "display-message" => {
                let at = self.target(t, Level::Pane)?;
                let default = "[#{session_name}] #{window_index}:#{window_name}, current pane #{pane_index} (#{pane_address})";
                let message = if o.words.is_empty() { o.get('F').unwrap_or(default).to_string() } else { o.words.join(" ") };
                println!("{}", self.expand(&message, &at));
            }
            "list-sessions" => {
                let places: Vec<Place> = (0..self.state.workspaces.len()).map(|ws| self.in_workspace(ws)).collect();
                self.show(&places, &o, "#{session_name}: #{session_windows} windows (n#{session_number})#{?session_attached, (attached),}");
            }
            "list-windows" => {
                let format = "#{window_index}: #{window_name}#{window_flags} (#{window_panes} panes) #{window_id}#{?window_active, (active),}";
                if o.has('a') {
                    self.show(&self.every_tab(), &o, &format!("#{{session_name}}:{format}"));
                } else {
                    self.show(&self.tabs_of(self.target(t, Level::Session)?.ws), &o, format);
                }
            }
            "list-panes" => {
                let format = "#{pane_index}: #{pane_address} #{pane_current_command} #{pane_state} \"#{pane_title}\" #{pane_id}#{?pane_active, (active),}";
                if o.has('a') {
                    self.show(&self.each_pane(&self.every_tab()), &o, &format!("#{{session_name}}:#{{window_index}}.{format}"));
                } else if o.has('s') {
                    let tabs = self.tabs_of(self.target(t, Level::Session)?.ws);
                    self.show(&self.each_pane(&tabs), &o, &format!("#{{window_index}}.{format}"));
                } else {
                    self.show(&self.each_pane(&[self.target(t, Level::Window)?]), &o, format);
                }
            }
            "list-commands" => {
                for (name, alias, _, usage) in COMMANDS {
                    let short = if alias.is_empty() { String::new() } else { format!(" ({alias})") };
                    println!("{name}{short} {usage}");
                }
            }
            "set-buffer" => self.keep(o.get('b'), &o.words.join(" "), o.has('a'))?,
            "load-buffer" => {
                let mut text = String::new();
                match named()?.as_str() {
                    "-" => std::io::stdin().read_to_string(&mut text).map(drop),
                    path => std::fs::read_to_string(path).map(|read| text = read),
                }
                .map_err(|e| e.to_string())?;
                self.keep(o.get('b'), &text, false)?;
            }
            "save-buffer" => {
                let text = self.kept(o.get('b'))?.1;
                match named()?.as_str() {
                    "-" => print!("{text}"),
                    path => std::fs::OpenOptions::new()
                        .create(true)
                        .write(true)
                        .append(o.has('a'))
                        .truncate(!o.has('a'))
                        .open(path)
                        .and_then(|mut file| file.write_all(text.as_bytes()))
                        .map_err(|e| e.to_string())?,
                }
            }
            "show-buffer" => print!("{}", self.kept(o.get('b'))?.1),
            "delete-buffer" => std::fs::remove_file(self.kept(o.get('b'))?.0).map_err(|e| e.to_string())?,
            "list-buffers" => {
                for (_, path) in saved(&self.data_dir.join("buffers")) {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    let start: String = text.chars().take(50).collect();
                    println!("{}: {} bytes: {start:?}", path.file_name().unwrap_or_default().to_string_lossy(), text.len());
                }
            }
            "paste-buffer" => {
                let pane = self.target(t, Level::Pane)?.pane;
                let (path, text) = self.kept(o.get('b'))?;
                let text = if o.has('r') { text } else { text.replace("\r\n", "\r").replace('\n', "\r") };
                self.call(Op::SendText { pane, text }).await?;
                if o.has('d') {
                    std::fs::remove_file(path).map_err(|e| e.to_string())?;
                }
            }
            _ => return fail(format!("unknown command \"{name}\"; run: nc --help")),
        }
        Ok(())
    }
}

pub async fn run(cli: &mut Cli, name: &str, rest: &[String]) -> Result<(), String> {
    let mut line = vec![name.to_string()];
    line.extend_from_slice(rest);
    for part in line.split(|arg| arg == ";") {
        if let Some((name, raw)) = part.split_first() {
            cli.refresh().await?;
            cli.tmux(name, raw).await?;
        }
    }
    Ok(())
}
