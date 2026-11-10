//! Accepts clients on a local endpoint: a named pipe on Windows, a Unix
//! socket elsewhere. Nothing listens on the network.
//!
//! A client proves it is the same user by sending a token that the server
//! wrote to a file in its data directory, which only that user can read.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nmux_proto::{read_frame, write_frame, Event, Op, Reply, ReplyResult, Request, ServerMessage, PROTOCOL_VERSION};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{broadcast, mpsc};

use crate::hub::Hub;

/// Where the server can be reached; written next to `state.json`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Endpoint {
    /// Pipe name or socket path.
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
        // One pipe per data directory, so a test server never collides with the real one.
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
        // %APPDATA% is readable by its owner (and administrators) only.
        std::fs::write(path, body)
    }
}

/// Listen until the hub is told to shut down. Fails straight away if another
/// server already owns this data directory.
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
    // Let the reply to the shutdown request reach its client before the runtime goes away.
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
        // `first_pipe_instance` makes this fail if the name is already served,
        // which is both the single-instance check and a guard against a
        // squatter that created the pipe first.
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

/// How many messages may queue for one client before it is dropped. A window
/// that cannot keep up reconnects and repaints from a snapshot.
const CLIENT_QUEUE: usize = 4096;

async fn serve_client<S>(hub: Arc<Hub>, stream: S, token: String)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);

    // Nothing is served, and no event is sent, before a valid hello.
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
    // Subscribe before replying so the client misses nothing after its hello.
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
        loop {
            match events.recv().await {
                Ok(event) => {
                    let event: Event = (*event).clone();
                    if event_tx.try_send(ServerMessage::Event(event)).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) | Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut stop = hub.shutdown.subscribe();
    loop {
        tokio::select! {
            frame = read_frame::<_, Request>(&mut reader) => {
                let Ok(Some(request)) = frame else { break };
                let hub = hub.clone();
                // Starting a process blocks for a moment; keep it off the async workers.
                let result = tokio::task::spawn_blocking(move || hub.handle(request.op)).await;
                let result = match result {
                    Ok(r) => r,
                    Err(_) => Err("internal error".to_string()),
                };
                let reply = ServerMessage::Reply { id: request.id, result: result.map_or_else(ReplyResult::Err, ReplyResult::Ok) };
                if tx.send(reply).await.is_err() {
                    break;
                }
            }
            // The event side gave up (the client fell too far behind).
            _ = tx.closed() => break,
            // Let go of the pipe so the next server can claim the name.
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
