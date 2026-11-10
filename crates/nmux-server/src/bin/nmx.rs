use std::path::PathBuf;
use std::time::{Duration, Instant};

use nmux_core::{AgentState, Pane, PaneId, PaneKind, PaneSpec, PaneStatus, State};
use nmux_proto::{Op, Reply};
use nmux_server::client::Client;

const HELP: &str = r#"nmx - talk to the panes running in Nmux

Usage: nmx <command> [options]

  ls                       list workspaces and panes with their addresses
  whoami                   the address of the pane this runs in
  send <to> <text>         type text into a pane and press enter
                           (--raw: no "from" line, --no-enter: leave it unsent)
  keys <to> <keys...>      press keys: enter, esc, tab, up, down, ctrl+c, shift+tab
  read <to>                print what is on a pane's screen
  wait <to>                wait until its agent stops working (--timeout <seconds>)
  resume <to>              start a stopped pane again
  new <to> [--shell]       open a new tab in that workspace (Claude by default)
  api '<json>'             send one raw request to the server

<to> is an address from "nmx ls": n8 is workspace 8 (its focused pane),
n8.2 is the second pane in it. A workspace name works too: "TL education".

Agents: ask another agent with
  nmx send n8 "your question"
A message sent from a pane to an agent starts with who sent it and how to reply.
Then wait for the answer to arrive in your own pane, or check with
  nmx wait n8 && nmx read n8
Options: --json (ls), --data-dir <dir>."#;

fn fail<T>(why: impl Into<String>) -> Result<T, String> {
    Err(why.into())
}

struct Cli {
    client: Client,
    state: State,
}

impl Cli {
    async fn call(&self, op: Op) -> Result<Reply, String> {
        self.client.call(op).await
    }

    fn me(&self) -> Option<PaneId> {
        let id = PaneId(std::env::var("NMUX_PANE_ID").ok()?.parse().ok()?);
        self.state.panes.contains_key(&id).then_some(id)
    }

    fn target(&self, args: &[String]) -> Result<PaneId, String> {
        let address = args.first().ok_or("say which pane: an address like n8 (see: nmx ls)")?;
        self.state.resolve(address)
    }

    fn list(&self, json: bool) {
        let me = self.me();
        if json {
            let rows: Vec<serde_json::Value> = self
                .state
                .workspaces
                .iter()
                .flat_map(|w| w.panes().into_iter().map(move |p| (w, p)))
                .filter_map(|(w, id)| {
                    let p = self.state.panes.get(&id)?;
                    Some(serde_json::json!({
                        "address": self.state.address(id), "workspace": w.name, "name": p.name, "kind": p.kind,
                        "status": p.status, "agent": p.agent, "cwd": p.cwd, "me": me == Some(id),
                    }))
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
            return;
        }
        for w in &self.state.workspaces {
            println!("n{:<5} {}  {}", w.number, w.name, w.cwd);
            let panes = w.panes();
            for id in &panes {
                let Some(p) = self.state.panes.get(id) else { continue };
                let state = pane_state(p);
                let kind = format!("{:?}", p.kind).to_lowercase();
                let address = self.state.address(*id).unwrap_or_default();
                let mark = if me == Some(*id) { "  <- you" } else { "" };
                println!("  {address:<7} {kind:<7} {state:<8} {}{mark}", p.name);
            }
        }
    }

    async fn send(&self, args: &[String]) -> Result<(), String> {
        let raw = args.iter().any(|a| a == "--raw");
        let enter = !args.iter().any(|a| a == "--no-enter");
        let words: Vec<&String> = args.iter().filter(|a| !matches!(a.as_str(), "--raw" | "--no-enter")).collect();
        let pane = self.state.resolve(words.first().ok_or("usage: nmx send <to> <text>")?)?;
        let body = words[1..].iter().map(|w| w.as_str()).collect::<Vec<_>>().join(" ");
        if body.trim().is_empty() {
            return fail("there is no text to send");
        }
        let to_agent = self.state.panes.get(&pane).is_some_and(|p| p.kind == PaneKind::Claude);
        let text = match self.me().filter(|_| !raw && to_agent).and_then(|me| Some((self.state.address(me)?, me))) {
            Some((from, me)) => {
                let name = self.state.workspaces.iter().find(|w| w.panes().contains(&me)).map(|w| w.name.as_str()).unwrap_or("");
                format!("[from {from} ({name}); reply with: nmx send {from} \"...\"] {body}")
            }
            None => body,
        };
        self.call(Op::SendText { pane, text }).await?;
        if enter {
            tokio::time::sleep(Duration::from_millis(250)).await;
            self.call(Op::Write { pane, data: "\r".into() }).await?;
        }
        Ok(())
    }

    async fn wait(&self, args: &[String]) -> Result<(), String> {
        let pane = self.target(args)?;
        let timeout = args.iter().position(|a| a == "--timeout").and_then(|i| args.get(i + 1)?.parse::<u64>().ok()).unwrap_or(600);
        let started = Instant::now();
        let mut seen_working = false;
        loop {
            let Reply::State { state } = self.call(Op::GetState).await? else { return fail("unexpected reply") };
            let p = state.panes.get(&pane).ok_or("that pane was closed")?;
            let working = p.status == PaneStatus::Running && p.agent == AgentState::Working;
            seen_working |= working;
            if !working && (seen_working || started.elapsed() > Duration::from_secs(4)) {
                println!("{}", pane_state(p));
                return Ok(());
            }
            if started.elapsed() > Duration::from_secs(timeout) {
                return fail(format!("still working after {timeout} seconds"));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
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
    if let Some(letter) = lower.strip_prefix("ctrl+").filter(|l| l.len() == 1 && l.as_bytes()[0].is_ascii_lowercase()) {
        return ((letter.as_bytes()[0] - b'a' + 1) as char).to_string();
    }
    match lower.as_str() {
        "enter" | "return" => "\r",
        "esc" | "escape" => "\x1b",
        "tab" => "\t",
        "shift+tab" => "\x1b[Z",
        "space" => " ",
        "backspace" => "\x7f",
        "up" => "\x1b[A",
        "down" => "\x1b[B",
        "right" => "\x1b[C",
        "left" => "\x1b[D",
        _ => key,
    }
    .to_string()
}

async fn run(mut args: Vec<String>) -> Result<(), String> {
    let data_dir = match args.iter().position(|a| a == "--data-dir") {
        Some(i) if i + 1 < args.len() => {
            let dir = PathBuf::from(args.remove(i + 1));
            args.remove(i);
            Some(dir)
        }
        _ => std::env::var_os("NMUX_DATA_DIR").map(PathBuf::from).or_else(nmux_server::default_data_dir),
    }
    .ok_or("cannot find the Nmux data directory; pass --data-dir")?;
    let command = args.first().cloned().unwrap_or_default();
    if matches!(command.as_str(), "" | "help" | "--help" | "-h") {
        println!("{HELP}");
        return Ok(());
    }
    let rest = &args[1..];

    let (client, _events) = nmux_server::client::connect(&data_dir).await.map_err(|e| format!("Nmux is not running ({e})"))?;
    if command == "api" {
        let op: Op = serde_json::from_str(rest.first().ok_or("usage: nmx api '<json request>'")?).map_err(|e| format!("bad request: {e}"))?;
        if matches!(op, Op::Hello { .. }) {
            return fail("hello is sent for you");
        }
        let reply = client.call(op).await?;
        println!("{}", serde_json::to_string_pretty(&reply).map_err(|e| e.to_string())?);
        return Ok(());
    }
    let Reply::State { state } = client.call(Op::GetState).await? else { return fail("unexpected reply") };
    let cli = Cli { client, state };

    match command.as_str() {
        "ls" | "list" => cli.list(rest.iter().any(|a| a == "--json")),
        "whoami" => {
            let me = cli.me().ok_or("this is not running inside an Nmux pane")?;
            println!("{}", cli.state.address(me).unwrap_or_default());
        }
        "send" => cli.send(rest).await?,
        "keys" => {
            let pane = cli.target(rest)?;
            let data: String = rest[1..].iter().map(|k| key_bytes(k)).collect();
            if data.is_empty() {
                return fail("usage: nmx keys <to> <keys...>");
            }
            cli.call(Op::Write { pane, data }).await?;
        }
        "read" => {
            let pane = cli.target(rest)?;
            let Reply::Text { text } = cli.call(Op::ReadPane { pane }).await? else { return fail("unexpected reply") };
            println!("{}", text.trim_end());
        }
        "wait" => cli.wait(rest).await?,
        "resume" => {
            let pane = cli.target(rest)?;
            cli.call(Op::RestartPane { pane }).await?;
        }
        "new" => {
            let pane = cli.target(rest)?;
            let workspace = cli.state.workspaces.iter().find(|w| w.panes().contains(&pane)).map(|w| w.id).ok_or("no such workspace")?;
            let kind = if rest.iter().any(|a| a == "--shell") { PaneKind::Shell } else { PaneKind::Claude };
            cli.call(Op::CreateTab { workspace, name: None, pane: PaneSpec::new(kind) }).await?;
            let Reply::State { state } = cli.call(Op::GetState).await? else { return fail("unexpected reply") };
            let made = state.workspaces.iter().find(|w| w.id == workspace).and_then(|w| w.focused_pane()).and_then(|p| state.address(p));
            println!("{}", made.unwrap_or_default());
        }
        other => return fail(format!("unknown command \"{other}\"; run: nmx --help")),
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(why) = run(std::env::args().skip(1).collect()).await {
        eprintln!("nmx: {why}");
        std::process::exit(1);
    }
}
