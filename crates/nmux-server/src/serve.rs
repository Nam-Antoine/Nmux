use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nmux_proto::{read_frame, write_frame, Event, Op, Reply, ReplyResult, Request, ServerMessage, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc};

use crate::hub::Hub;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    pub address: String,
    pub token: String,
    pub pid: u32,
}

pub fn endpoint_file(data_dir: &Path) -> PathBuf {
    data_dir.join("endpoint.json")
}

pub fn read_endpoint(data_dir: &Path) -> Option<Endpoint> {
    serde_json::from_slice(&std::fs::read(endpoint_file(data_dir)).ok()?).ok()
}

fn address_for(data_dir: &Path) -> String {
    if cfg!(windows) {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        data_dir.to_string_lossy().to_lowercase().hash(&mut hasher);
        format!(r"\\.\pipe\nmux-{:016x}", hasher.finish())
    } else {
        data_dir.join("nmux.sock").to_string_lossy().into_owned()
    }
}

fn new_token() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn write_private(path: &Path, body: &[u8]) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;
        f.write_all(body)
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, body)
    }
}

pub async fn run(hub: Arc<Hub>, data_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let endpoint = Endpoint { address: address_for(data_dir), token: new_token()?, pid: std::process::id() };
    let listener = Listener::bind(&endpoint.address)?;
    write_private(&endpoint_file(data_dir), &serde_json::to_vec_pretty(&endpoint)?)?;

    let mut stop = hub.shutdown.subscribe();
    let result = tokio::select! {
        r = listener.accept_loop(hub.clone(), endpoint.token.clone()) => r,
        _ = stop.wait_for(|stopping| *stopping) => Ok(()),
    };
    let _ = std::fs::remove_file(endpoint_file(data_dir));
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    result
}

#[cfg(windows)]
struct Listener {
    address: String,
    first: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl Listener {
    fn bind(address: &str) -> anyhow::Result<Listener> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let first = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(address)
            .map_err(|e| anyhow::anyhow!("another nmux-server is already running here ({e})"))?;
        Ok(Listener { address: address.to_string(), first })
    }

    async fn accept_loop(self, hub: Arc<Hub>, token: String) -> anyhow::Result<()> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let mut server = self.first;
        loop {
            server.connect().await?;
            let next = ServerOptions::new().reject_remote_clients(true).create(&self.address)?;
            let stream = std::mem::replace(&mut server, next);
            tokio::spawn(serve_client(hub.clone(), stream, token.clone()));
        }
    }
}

#[cfg(unix)]
struct Listener {
    inner: tokio::net::UnixListener,
}

#[cfg(unix)]
impl Listener {
    fn bind(address: &str) -> anyhow::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        if std::os::unix::net::UnixStream::connect(address).is_ok() {
            anyhow::bail!("another nmux-server is already running here");
        }
        let _ = std::fs::remove_file(address);
        let inner = tokio::net::UnixListener::bind(address)?;
        std::fs::set_permissions(address, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener { inner })
    }

    async fn accept_loop(self, hub: Arc<Hub>, token: String) -> anyhow::Result<()> {
        loop {
            let (stream, _) = self.inner.accept().await?;
            tokio::spawn(serve_client(hub.clone(), stream, token.clone()));
        }
    }
}

const OUTPUT_FRAME: Duration = Duration::from_millis(6);
const OUTPUT_MERGE_BYTES: usize = 256 * 1024;

fn may_wait(op: &Op) -> bool {
    matches!(
        op,
        Op::CheckMachine { .. }
            | Op::ListMachinePorts { .. }
            | Op::GitChanges { .. }
            | Op::GitDiff { .. }
            | Op::ListPorts
            | Op::ListSshHosts
            | Op::ListDir { .. }
            | Op::ReadFile { .. }
            | Op::CheckPath { .. }
            | Op::PasteImage { .. }
            | Op::StartForward { .. }
    )
}

const CLIENT_QUEUE: usize = 4096;

async fn serve_client<S>(hub: Arc<Hub>, stream: S, token: String)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    let hello: Option<Request> = read_frame(&mut reader).await.ok().flatten();
    let Some(Request { id, op: Op::Hello { token: given, protocol } }) = hello else { return };
    let verdict = if !same_token(&given, &token) {
        Err("wrong token".to_string())
    } else if protocol != PROTOCOL_VERSION {
        Err(format!("protocol {protocol} is not supported (server speaks {PROTOCOL_VERSION})"))
    } else {
        Ok(Reply::Hello {
            protocol: PROTOCOL_VERSION,
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            pid: std::process::id(),
        })
    };
    let accepted = verdict.is_ok();
    let mut events = hub.subscribe();
    let reply = ServerMessage::Reply { id, result: verdict.map_or_else(ReplyResult::Err, ReplyResult::Ok) };
    if write_frame(&mut writer, &reply).await.is_err() || !accepted {
        return;
    }

    let (tx, mut outbox) = mpsc::channel::<ServerMessage>(CLIENT_QUEUE);
    let write_task = tokio::spawn(async move {
        while let Some(message) = outbox.recv().await {
            if write_frame(&mut writer, &message).await.is_err() {
                break;
            }
        }
    });

    let event_tx = tx.clone();
    let event_task = tokio::spawn(async move {
        let mut held: Option<Arc<Event>> = None;
        let mut last_output: Option<Instant> = None;
        'events: loop {
            let arc = match held.take() {
                Some(event) => event,
                None => match events.recv().await {
                    Ok(event) => event,
                    Err(_) => break,
                },
            };
            let mut event: Event = (*arc).clone();
            if let Event::Output { pane, seq, data } = &mut event {
                if let Some(wait) = last_output.and_then(|at| OUTPUT_FRAME.checked_sub(at.elapsed())) {
                    tokio::time::sleep(wait).await;
                }
                while data.len() < OUTPUT_MERGE_BYTES {
                    match events.try_recv() {
                        Ok(more) => match &*more {
                            Event::Output { pane: p, seq: s, data: d } if p == pane => {
                                data.push_str(d);
                                *seq = *s;
                            }
                            _ => {
                                held = Some(more);
                                break;
                            }
                        },
                        Err(broadcast::error::TryRecvError::Empty) => break,
                        Err(_) => break 'events,
                    }
                }
                last_output = Some(Instant::now());
            }
            if event_tx.try_send(ServerMessage::Event(event)).is_err() {
                break;
            }
        }
    });

    let mut stop = hub.shutdown.subscribe();
    loop {
        tokio::select! {
            frame = read_frame::<_, Request>(&mut reader) => {
                let Ok(Some(request)) = frame else { break };
                let hub = hub.clone();
                let waits = may_wait(&request.op);
                let id = request.id;
                let work = tokio::task::spawn_blocking(move || hub.handle(request.op));
                let answer = move |result: Result<Result<Reply, String>, tokio::task::JoinError>| {
                    let result = result.unwrap_or_else(|_| Err("internal error".to_string()));
                    ServerMessage::Reply { id, result: result.map_or_else(ReplyResult::Err, ReplyResult::Ok) }
                };
                if waits {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(answer(work.await)).await;
                    });
                } else if tx.send(answer(work.await)).await.is_err() {
                    break;
                }
            }
            _ = tx.closed() => break,
            _ = stop.changed() => break,
        }
        if event_task.is_finished() {
            break;
        }
    }
    event_task.abort();
    drop(tx);
    let _ = write_task.await;
}
