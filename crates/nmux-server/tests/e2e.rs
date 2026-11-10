//! A real server, a real client, a real shell.

use std::path::Path;
use std::time::Duration;

use nmux_core::{Dir, PaneId, PaneKind, PaneSpec, PaneStatus, State};
use nmux_proto::{Event, Op, Reply};
use nmux_server::client::{self, Client};
use nmux_server::hub::Config;
use nmux_server::launch;
use tokio::sync::mpsc::UnboundedReceiver;

fn config(dir: &Path) -> Config {
    Config { data_dir: dir.to_path_buf(), env: launch::process_env(), claude_dir: None, legacy_sessions: None }
}

async fn start(dir: &Path) -> (tokio::task::JoinHandle<anyhow::Result<()>>, Client, UnboundedReceiver<Event>) {
    let server = tokio::spawn(nmux_server::run(config(dir)));
    for _ in 0..100 {
        if let Ok((client, events)) = client::connect(dir).await {
            return (server, client, events);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server did not come up");
}

/// Collect a pane's output until `needle` shows up.
async fn wait_output(events: &mut UnboundedReceiver<Event>, pane: PaneId, needle: &str) -> String {
    let mut seen = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while !seen.contains(needle) {
        match tokio::time::timeout_at(deadline, events.recv()).await {
            Ok(Some(Event::Output { pane: p, data, .. })) if p == pane => seen.push_str(&data),
            Ok(Some(_)) => {}
            Ok(None) => panic!("connection closed; saw: {seen:?}"),
            Err(_) => panic!("timed out waiting for {needle:?}; saw: {seen:?}"),
        }
    }
    seen
}

async fn state(client: &Client) -> State {
    match client.call(Op::GetState).await.unwrap() {
        Reply::State { state } => state,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_shell_pane_runs_survives_reattach_and_is_restored() {
    let dir = tempfile::tempdir().unwrap();
    let cwd = dir.path().to_string_lossy().into_owned();
    let (server, client, mut events) = start(dir.path()).await;

    // A second server on the same data directory is refused.
    assert!(nmux_server::run(config(dir.path())).await.is_err());

    // Bad input never reaches a process.
    let bad = Op::CreateWorkspace { name: None, cwd: "relative".into(), pane: PaneSpec::new(PaneKind::Shell) };
    assert!(client.call(bad).await.is_err());
    let missing = Op::CreateWorkspace {
        name: None,
        cwd: dir.path().join("nope").to_string_lossy().into_owned(),
        pane: PaneSpec::new(PaneKind::Shell),
    };
    assert!(client.call(missing).await.unwrap_err().contains("does not exist"));

    let created = match client
        .call(Op::CreateWorkspace { name: Some("demo".into()), cwd: cwd.clone(), pane: PaneSpec::new(PaneKind::Shell) })
        .await
        .unwrap()
    {
        Reply::Created { created } => created,
        other => panic!("{other:?}"),
    };
    let pane = created.pane;
    assert_eq!(state(&client).await.panes[&pane].status, PaneStatus::Running);

    // Type a command; the marker is computed so the echo of the keystrokes cannot match.
    client.call(Op::Write { pane, data: "echo nmux-$(6*7)-ok\r".into() }).await.unwrap();
    wait_output(&mut events, pane, "nmux-42-ok").await;

    // A second client sees the same screen from a snapshot.
    let (other, _other_events) = client::connect(dir.path()).await.unwrap();
    match other.call(Op::Attach { pane }).await.unwrap() {
        Reply::Attached { snapshot } => {
            assert!(snapshot.data.contains("nmux-42-ok"), "{:?}", snapshot.data);
            assert!(snapshot.seq > 0);
        }
        other => panic!("{other:?}"),
    }

    // Split, resize, and close the new pane again.
    let split = match client.call(Op::SplitPane { target: pane, dir: Dir::Right, pane: PaneSpec::new(PaneKind::Shell) }).await.unwrap() {
        Reply::Created { created } => created.pane,
        other => panic!("{other:?}"),
    };
    client.call(Op::Resize { pane: split, cols: 90, rows: 20 }).await.unwrap();
    assert!(client.call(Op::Resize { pane: split, cols: 0, rows: 20 }).await.is_err());
    assert_eq!(state(&client).await.workspaces[0].tabs[0].layout.panes(), vec![pane, split]);
    client.call(Op::ClosePane { pane: split }).await.unwrap();
    assert_eq!(state(&client).await.panes.len(), 1);

    // The shell exits: the pane stays, marked exited, and can be started again.
    client.call(Op::Write { pane, data: "exit\r".into() }).await.unwrap();
    for _ in 0..100 {
        if state(&client).await.panes[&pane].status == PaneStatus::Exited {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(state(&client).await.panes[&pane].status, PaneStatus::Exited);
    client.call(Op::RestartPane { pane }).await.unwrap();
    assert_eq!(state(&client).await.panes[&pane].status, PaneStatus::Running);
    client.call(Op::Write { pane, data: "echo again-$(1+1)\r".into() }).await.unwrap();
    wait_output(&mut events, pane, "again-2").await;

    // Shutdown stops the server; a new one restores the layout with nothing running.
    client.call(Op::Shutdown).await.unwrap();
    tokio::time::timeout(Duration::from_secs(10), server).await.unwrap().unwrap().unwrap();
    assert!(client::connect(dir.path()).await.is_err());

    let (server, client, _events) = start(dir.path()).await;
    let restored = state(&client).await;
    assert_eq!(restored.workspaces.len(), 1);
    assert_eq!(restored.workspaces[0].name, "demo");
    assert_eq!(restored.panes[&pane].status, PaneStatus::Exited);
    client.call(Op::Shutdown).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wrong_token_is_turned_away() {
    let dir = tempfile::tempdir().unwrap();
    let (server, client, _events) = start(dir.path()).await;

    let file = nmux_server::serve::endpoint_file(dir.path());
    let mut endpoint = nmux_server::serve::read_endpoint(dir.path()).unwrap();
    let real = endpoint.token.clone();
    endpoint.token = "0".repeat(real.len());
    std::fs::write(&file, serde_json::to_vec(&endpoint).unwrap()).unwrap();
    let err = client::connect(dir.path()).await.err().expect("must be refused");
    assert!(err.to_string().contains("wrong token"), "{err}");

    client.call(Op::Shutdown).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
}

/// A port opened by something running in a pane is listed under that pane,
/// and files are only served from inside the workspace.
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread")]
async fn ports_and_files_belong_to_their_workspace() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::write(project.join("src").join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(dir.path().join("outside.txt"), "private").unwrap();
    let (server, client, mut events) = start(dir.path()).await;

    let created = match client
        .call(Op::CreateWorkspace { name: None, cwd: project.to_string_lossy().into_owned(), pane: PaneSpec::new(PaneKind::Shell) })
        .await
        .unwrap()
    {
        Reply::Created { created } => created,
        other => panic!("{other:?}"),
    };
    let pane = created.pane;

    match client.call(Op::ListDir { path: project.to_string_lossy().into_owned() }).await.unwrap() {
        Reply::Dir { entries, .. } => assert_eq!(entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["src"]),
        other => panic!("{other:?}"),
    }
    let outside = Op::ReadFile { path: dir.path().join("outside.txt").to_string_lossy().into_owned() };
    assert!(client.call(outside).await.unwrap_err().contains("outside"));

    // Listen on a free port from inside the shell and print which one.
    let script = "$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,0);$l.Start();'po'+'rt='+$l.LocalEndpoint.Port+'=o'+'k'\r";
    client.call(Op::Write { pane, data: script.into() }).await.unwrap();
    let seen = wait_output(&mut events, pane, "=ok").await;
    let port: u16 = seen.rsplit("port=").next().unwrap().split('=').next().unwrap().parse().unwrap();

    let ports = match client.call(Op::ListPorts).await.unwrap() {
        Reply::Ports { ports } => ports,
        other => panic!("{other:?}"),
    };
    let mine = ports.iter().find(|p| p.port == port).unwrap_or_else(|| panic!("port {port} not in {} ports; seen {seen:?}", ports.len()));
    assert_eq!(mine.pane, Some(pane), "{mine:?}");
    assert_eq!(ports[0].pane, Some(pane), "ports of panes come first");

    client.call(Op::Shutdown).await.unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(10), server).await;
}
