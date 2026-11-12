use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use nmux_core::{AgentState, Created, Pane, PaneId, PaneKind, PaneSpec, PaneStatus, State};
use nmux_proto::{Op, Reply};
use nmux_server::client::Client;
use nmux_server::screen::SCROLLBACK_LINES;

mod tmux;

const HELP: &str = r#"nc - run and talk to the panes in Nmux (this is not netcat)

Usage: nc <command> [options]
       nc               list the panes

Panes
  run <claude|shell|cmd> [task]   open it in a new tab and wait like a shell
                              (--bg: don't wait, --in <t>: in that workspace)
  ls                          list running panes (--all: stopped too)
  get [t]                     show one pane
  attach [t]                  bring it to the front in the Nmux window
  read [t]                    print its screen
  kill <t>                    end its program
  rm <t>                      remove a stopped pane
  resume <t>                  start a stopped pane again

Talk to programs
  send <t> <text>             paste text and press enter
                              (--raw: no "from" line, --no-enter: leave it unsent)
  keys <t> <keys...>          press keys: enter, esc, tab, up, ctrl+c, shift+tab
  interrupt [t]               press the stop key, report what happened
  wait [t]                    wait until it stops working (--output X: until it
                              prints X, --timeout <seconds>). Use it instead of sleep.

tmux commands
  tmux's commands work under their own names and flags: new-session,
  new-window, split-window, kill-pane, select-pane, send-keys, capture-pane,
  list-panes, display-message, paste-buffer and more (nc list-commands).
  A session is a workspace and a window is a tab. -t takes an address like
  n8b, or tmux's session:window.pane, %pane, @window and $session.
  ls, send, run and wait keep the meaning above: for tmux's, write
  list-sessions, send-keys.
  Every pane also has a tmux command that is this same tool, and the TMUX
  and TMUX_PANE variables, so what is written for tmux drives Nmux.

Other machines
  Nmux puts nc on every machine it knows, in ~/.nmux/bin. In a pane there
  it is this same tool, talking to the Nmux on your computer. Outside a
  pane it runs the machine's own nc.

More
  whoami                      the address of the pane this runs in
  api '<json>'                one raw request to the server

<t> is an address like n8 or n8b, or a workspace name. n8 is workspace 8;
n8b is the second pane in it. In a pane, get, attach, read, wait and
interrupt pick that pane when <t> is left out.
Options: --json (default when piped; read prints text), --data-dir <dir>.

Examples
  nc send n8 "what port does the API use?"   ask the agent in workspace 8
  nc wait n8 && nc read n8                    wait for it, read its answer
  nc run --bg claude "review src/x.rs"        a helper agent in a new tab
A message sent from a pane to an agent starts with who sent it and how to reply."#;

const FLAGS_WITH_VALUE: [&str; 3] = ["--in", "--output", "--timeout"];

fn fail<T>(why: impl Into<String>) -> Result<T, String> {
    Err(why.into())
}

fn pane_state(pane: &Pane) -> String {
    match (pane.status, pane.agent) {
        (PaneStatus::Exited, _) => "stopped".into(),
        (_, AgentState::Unknown) => "running".into(),
        (_, agent) => format!("{agent:?}").to_lowercase(),
    }
}

fn key_bytes(key: &str) -> String {
    let lower = key.to_lowercase();
    let held = |long: &str, short: &str| lower.strip_prefix(long).or(lower.strip_prefix(short)).map(|rest| &key[key.len() - rest.len()..]);
    if let Some(letter) = held("ctrl+", "c-").filter(|l| l.len() == 1 && l.as_bytes()[0].is_ascii_alphabetic()) {
        return ((letter.as_bytes()[0].to_ascii_lowercase() - b'a' + 1) as char).to_string();
    }
    if let Some(rest) = held("alt+", "m-").filter(|rest| !rest.is_empty()) {
        return format!("\x1b{}", key_bytes(rest));
    }
    const FUNCTION: [&str; 12] = ["OP", "OQ", "OR", "OS", "[15~", "[17~", "[18~", "[19~", "[20~", "[21~", "[23~", "[24~"];
    if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=12).contains(n)) {
        return format!("\x1b{}", FUNCTION[n - 1]);
    }
    match lower.as_str() {
        "enter" | "return" => "\r",
        "esc" | "escape" => "\x1b",
        "tab" => "\t",
        "shift+tab" | "btab" => "\x1b[Z",
        "space" => " ",
        "backspace" | "bspace" => "\x7f",
        "home" => "\x1b[H",
        "end" => "\x1b[F",
        "pageup" | "pgup" | "ppage" => "\x1b[5~",
        "pagedown" | "pgdn" | "npage" => "\x1b[6~",
        "insert" | "ic" => "\x1b[2~",
        "delete" | "dc" => "\x1b[3~",
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        _ => key,
    }
    .to_string()
}

struct Args {
    words: Vec<String>,
    flags: Vec<String>,
    values: Vec<(String, String)>,
}

impl Args {
    fn parse(raw: &[String]) -> Args {
        let mut args = Args { words: Vec::new(), flags: Vec::new(), values: Vec::new() };
        let mut i = 0;
        while i < raw.len() {
            let a = &raw[i];
            if FLAGS_WITH_VALUE.contains(&a.as_str()) && i + 1 < raw.len() {
                args.values.push((a.clone(), raw[i + 1].clone()));
                i += 1;
            } else if a.starts_with("--") && a.len() > 2 {
                args.flags.push(a.clone());
            } else {
                args.words.push(a.clone());
            }
            i += 1;
        }
        args
    }

    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    fn value(&self, flag: &str) -> Option<&str> {
        self.values.iter().find(|(f, _)| f == flag).map(|(_, v)| v.as_str())
    }

    fn json(&self) -> bool {
        self.has("--json") || !(std::io::stdout().is_terminal() || std::env::var_os("NMUX_TTY").is_some())
    }
}

struct Cli {
    client: Client,
    state: State,
    data_dir: PathBuf,
    as_tmux: bool,
}

impl Cli {
    async fn call(&self, op: Op) -> Result<Reply, String> {
        self.client.call(op).await
    }

    async fn refresh(&mut self) -> Result<(), String> {
        let Reply::State { state } = self.call(Op::GetState).await? else { return fail("unexpected reply") };
        self.state = state;
        Ok(())
    }

    fn me(&self) -> Option<PaneId> {
        let id = PaneId(std::env::var("NMUX_PANE_ID").ok()?.parse().ok()?);
        self.state.panes.contains_key(&id).then_some(id)
    }

    fn named(&self, address: Option<&String>) -> Result<PaneId, String> {
        self.state.resolve(address.ok_or("say which pane: an address like n8 (see: nc ls)")?)
    }

    fn named_or_me(&self, address: Option<&String>) -> Result<PaneId, String> {
        match address {
            Some(a) => self.state.resolve(a),
            None => self.me().ok_or_else(|| "say which pane: an address like n8 (see: nc ls)".to_string()),
        }
    }

    async fn open(&mut self, op: Op) -> Result<Created, String> {
        let Reply::Created { created } = self.call(op).await? else { return fail("unexpected reply") };
        self.refresh().await?;
        Ok(created)
    }

    async fn rows(&self, pane: PaneId, history: bool) -> Result<(Vec<String>, usize), String> {
        let Reply::Attached { snapshot } = self.call(Op::Attach { pane }).await? else { return fail("unexpected reply") };
        let mut parser = vt100::Parser::new(snapshot.rows, snapshot.cols, if history { SCROLLBACK_LINES } else { 0 });
        parser.process(snapshot.data.as_bytes());
        parser.screen_mut().set_scrollback(usize::MAX);
        let above = parser.screen().scrollback();
        let mut lines: Vec<String> = Vec::new();
        for back in (1..=above).rev() {
            parser.screen_mut().set_scrollback(back);
            lines.extend(parser.screen().rows(0, snapshot.cols).next());
        }
        parser.screen_mut().set_scrollback(0);
        lines.extend(parser.screen().rows(0, snapshot.cols));
        Ok((lines.iter().map(|line| line.trim_end().to_string()).collect(), above))
    }

    async fn screen(&self, pane: PaneId) -> Result<String, String> {
        let mut lines: Vec<String> = Vec::new();
        for line in self.rows(pane, false).await?.0 {
            if !(line.is_empty() && lines.last().is_none_or(|l| l.is_empty())) {
                lines.push(line);
            }
        }
        Ok(lines.join("\n").trim().to_string())
    }

    fn pane(&self, id: PaneId) -> Result<&Pane, String> {
        self.state.panes.get(&id).ok_or_else(|| "that pane was closed".to_string())
    }

    fn describe(&self, id: PaneId) -> serde_json::Value {
        let Some(p) = self.state.panes.get(&id) else { return serde_json::Value::Null };
        let workspace = self.state.workspaces.iter().find(|w| w.panes().contains(&id)).map(|w| w.name.as_str());
        serde_json::json!({
            "address": self.state.address(id), "workspace": workspace, "name": p.name, "kind": p.shown_kind(),
            "state": pane_state(p), "cwd": p.cwd, "me": self.me() == Some(id),
        })
    }

    fn list(&self, args: &Args) {
        let all = args.has("--all");
        let shown = |id: &PaneId| all || self.state.panes.get(id).is_some_and(|p| p.status == PaneStatus::Running);
        if args.json() {
            let rows: Vec<serde_json::Value> =
                self.state.workspaces.iter().flat_map(|w| w.panes()).filter(shown).map(|id| self.describe(id)).collect();
            println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
            return;
        }
        let me = self.me();
        for w in &self.state.workspaces {
            let panes: Vec<PaneId> = w.panes().into_iter().filter(shown).collect();
            if panes.is_empty() {
                continue;
            }
            println!("n{:<5} {}  {}", w.number, w.name, w.cwd);
            for id in panes {
                let Some(p) = self.state.panes.get(&id) else { continue };
                let kind = format!("{:?}", p.shown_kind()).to_lowercase();
                let mark = if me == Some(id) { "  <- you" } else { "" };
                println!("  {:<7} {kind:<7} {:<8} {}{mark}", self.state.address(id).unwrap_or_default(), pane_state(p), p.name);
            }
        }
    }

    async fn paste(&self, pane: PaneId, text: String, enter: bool) -> Result<(), String> {
        self.call(Op::SendText { pane, text }).await?;
        if enter {
            tokio::time::sleep(Duration::from_millis(250)).await;
            self.call(Op::Write { pane, data: "\r".into() }).await?;
        }
        Ok(())
    }

    async fn send(&self, args: &Args) -> Result<(), String> {
        let pane = self.named(args.words.first())?;
        let body = args.words[1..].join(" ");
        if body.trim().is_empty() {
            return fail("there is no text to send");
        }
        let to_agent = self.pane(pane)?.shown_kind() == PaneKind::Claude;
        let from = self.me().filter(|_| to_agent && !args.has("--raw")).and_then(|me| Some((self.state.address(me)?, me)));
        let text = match from {
            Some((from, me)) => {
                let name = self.state.workspaces.iter().find(|w| w.panes().contains(&me)).map(|w| w.name.as_str()).unwrap_or("");
                format!("[from {from} ({name}); reply with: nc send {from} \"...\"] {body}")
            }
            None => body,
        };
        self.paste(pane, text, !args.has("--no-enter")).await
    }

    async fn wait(&mut self, pane: PaneId, args: &Args) -> Result<String, String> {
        let timeout = Duration::from_secs(args.value("--timeout").and_then(|t| t.parse().ok()).unwrap_or(600));
        let output = args.value("--output");
        let started = Instant::now();
        let mut seen_working = false;
        loop {
            self.refresh().await?;
            let p = self.pane(pane)?;
            let state = pane_state(p);
            if let Some(wanted) = output {
                if p.status == PaneStatus::Exited || self.screen(pane).await?.contains(wanted) {
                    return Ok(state);
                }
            } else {
                let working = p.status == PaneStatus::Running && p.agent == AgentState::Working;
                seen_working |= working;
                if !working && (seen_working || started.elapsed() > Duration::from_secs(4)) {
                    return Ok(state);
                }
            }
            if started.elapsed() > timeout {
                return fail(format!("still going after {} seconds", timeout.as_secs()));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    async fn type_when_ready(&mut self, pane: PaneId, kind: PaneKind, line: String) -> Result<(), String> {
        if line.trim().is_empty() {
            return Ok(());
        }
        let ready = Instant::now();
        while kind == PaneKind::Claude && ready.elapsed() < Duration::from_secs(40) {
            tokio::time::sleep(Duration::from_millis(500)).await;
            self.refresh().await?;
            if self.pane(pane)?.agent == AgentState::Idle {
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(if kind == PaneKind::Claude { 300 } else { 1200 })).await;
        self.paste(pane, line, true).await
    }

    async fn run(&mut self, args: &Args) -> Result<(), String> {
        let home = match args.value("--in") {
            Some(address) => self.state.resolve(address)?,
            None => self.me().ok_or("say where with --in <t>: this is not running inside an Nmux pane")?,
        };
        let workspace = self.state.workspaces.iter().find(|w| w.panes().contains(&home)).map(|w| w.id).ok_or("no such workspace")?;
        let program = args.words.first().map(String::as_str).unwrap_or("shell");
        let rest = args.words.get(1..).unwrap_or_default().join(" ");
        let (kind, line) = match program {
            "claude" => (PaneKind::Claude, rest),
            "shell" => (PaneKind::Shell, rest),
            _ => (PaneKind::Shell, args.words.join(" ")),
        };
        let pane = self.open(Op::CreateTab { workspace, name: None, pane: PaneSpec::new(kind) }).await?.pane;
        let address = self.state.address(pane).unwrap_or_default();
        self.type_when_ready(pane, kind, line).await?;
        if args.has("--bg") || kind == PaneKind::Shell {
            println!("{address}");
            return Ok(());
        }
        let state = self.wait(pane, args).await?;
        println!("{}\n\n{address} is {state}", self.screen(pane).await?);
        Ok(())
    }
}

const TMUX_FLAGS_WITH_VALUE: [&str; 5] = ["-c", "-f", "-L", "-S", "-T"];

fn as_tmux(raw: &mut Vec<String>) -> bool {
    let called = std::env::args().next().map(PathBuf::from);
    if !called.and_then(|exe| Some(exe.file_stem()?.to_string_lossy().eq_ignore_ascii_case("tmux"))).unwrap_or(false) {
        return false;
    }
    while raw.first().is_some_and(|a| a.starts_with('-') && a != "-V" && a != "-h") {
        let flag = raw.remove(0);
        if TMUX_FLAGS_WITH_VALUE.contains(&flag.as_str()) && !raw.is_empty() {
            raw.remove(0);
        }
    }
    if raw.is_empty() {
        raw.push("new-session".into());
    }
    true
}

async fn run(mut raw: Vec<String>) -> Result<(), String> {
    let as_tmux = as_tmux(&mut raw);
    if as_tmux && raw[0] == "-V" {
        println!("tmux 3.4");
        return Ok(());
    }
    let data_dir = match raw.iter().position(|a| a == "--data-dir") {
        Some(i) if i + 1 < raw.len() => {
            let dir = PathBuf::from(raw.remove(i + 1));
            raw.remove(i);
            Some(dir)
        }
        _ => std::env::var_os("NMUX_DATA_DIR").map(PathBuf::from).or_else(nmux_server::default_data_dir),
    }
    .ok_or("cannot find the Nmux data directory; pass --data-dir")?;
    let command = raw.first().cloned().unwrap_or_else(|| "ls".into());
    if matches!(command.as_str(), "help" | "--help" | "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let rest = raw.get(1..).unwrap_or_default();

    let (client, _events) = nmux_server::client::connect(&data_dir).await.map_err(|e| format!("Nmux is not running ({e})"))?;
    if command == "api" {
        let op: Op = serde_json::from_str(rest.first().ok_or("usage: nc api '<json request>'")?).map_err(|e| format!("bad request: {e}"))?;
        if matches!(op, Op::Hello { .. }) {
            return fail("hello is sent for you");
        }
        let reply = client.call(op).await?;
        println!("{}", serde_json::to_string_pretty(&reply).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let Reply::State { state } = client.call(Op::GetState).await? else { return fail("unexpected reply") };
    let mut cli = Cli { client, state, data_dir, as_tmux };
    if as_tmux {
        return tmux::run(&mut cli, &command, rest).await;
    }
    let args = Args::parse(rest);
    let first = args.words.first();

    match command.as_str() {
        "ls" | "list" => cli.list(&args),
        "whoami" => {
            let me = cli.me().ok_or("this is not running inside an Nmux pane")?;
            println!("{}", cli.state.address(me).unwrap_or_default());
        }
        "get" => {
            let pane = cli.named_or_me(first)?;
            if args.json() {
                println!("{}", serde_json::to_string_pretty(&cli.describe(pane)).unwrap_or_default());
            } else {
                let p = cli.pane(pane)?;
                println!("{}  {:?}  {}  {}  {}", cli.state.address(pane).unwrap_or_default(), p.shown_kind(), pane_state(p), p.name, p.cwd);
            }
        }
        "read" => {
            let pane = cli.named_or_me(first)?;
            println!("{}", cli.screen(pane).await?);
        }
        "kill" => {
            let pane = cli.named(first)?;
            cli.call(Op::StopPane { pane }).await?;
        }
        "rm" => {
            let pane = cli.named(first)?;
            if cli.pane(pane)?.status == PaneStatus::Running {
                return fail("it is still running; end it first: nc kill");
            }
            cli.call(Op::ClosePane { pane }).await?;
        }
        "resume" => {
            let pane = cli.named(first)?;
            cli.call(Op::RestartPane { pane }).await?;
        }
        "send" => cli.send(&args).await?,
        "keys" => {
            let pane = cli.named(first)?;
            let data: String = args.words[1..].iter().map(|k| key_bytes(k)).collect();
            if data.is_empty() {
                return fail("usage: nc keys <t> <keys...>");
            }
            cli.call(Op::Write { pane, data }).await?;
        }
        "interrupt" => {
            let pane = cli.named_or_me(first)?;
            let before = pane_state(cli.pane(pane)?);
            cli.call(Op::Write { pane, data: "\x1b".into() }).await?;
            tokio::time::sleep(Duration::from_millis(1500)).await;
            cli.refresh().await?;
            println!("was {before}, now {}", pane_state(cli.pane(pane)?));
        }
        "wait" => {
            let pane = cli.named_or_me(first)?;
            println!("{}", cli.wait(pane, &args).await?);
        }
        "run" => cli.run(&args).await?,
        other => tmux::run(&mut cli, other, rest).await?,
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(why) = run(std::env::args().skip(1).collect()).await {
        eprintln!("nc: {why}");
        std::process::exit(1);
    }
}
