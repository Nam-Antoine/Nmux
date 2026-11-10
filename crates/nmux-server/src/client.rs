use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmux_proto::{read_frame, write_frame, Event, Op, Reply, ReplyResult, Request, ServerMessage, PROTOCOL_VERSION};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};

use crate::serve::read_endpoint;

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<ReplyResult>>>>;

pub struct Client {
    requests: mpsc::Sender<Request>,
    pending: Pending,
    next_id: AtomicU64,
    pub server_pid: u32,
}

pub async fn connect(data_dir: &Path) -> std::io::Result<(Client, mpsc::UnboundedReceiver<Event>)> {
    let endpoint = read_endpoint(data_dir)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no nmux-server is running"))?;
    let stream = open(&endpoint.address).await?;
    handshake(stream, endpoint.token).await
}

#[cfg(windows)]
async fn open(address: &str) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    const ERROR_PIPE_BUSY: i32 = 231;
    let mut tries = 0;
    loop {
        match ClientOptions::new().open(address) {
            Ok(pipe) => return Ok(pipe),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) && tries < 40 => {
                tries += 1;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

#[cfg(unix)]
async fn open(address: &str) -> std::io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(address).await
}

async fn handshake<S>(stream: S, token: String) -> std::io::Result<(Client, mpsc::UnboundedReceiver<Event>)>
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
    write_frame(&mut writer, &Request { id: 0, op: Op::Hello { token, protocol: PROTOCOL_VERSION } }).await?;
    let hello = tokio::time::timeout(Duration::from_secs(5), read_frame::<_, ServerMessage>(&mut reader))
        .await
        .map_err(|_| std::io::Error::other("the server did not answer"))??;
    let server_pid = match hello {
        Some(ServerMessage::Reply { result: ReplyResult::Ok(Reply::Hello { pid, .. }), .. }) => pid,
        Some(ServerMessage::Reply { result: ReplyResult::Err(why), .. }) => return Err(std::io::Error::other(why)),
        _ => return Err(std::io::Error::other("unexpected answer to hello")),
    };

    let pending: Pending = Arc::default();
    let (requests, mut outbox) = mpsc::channel::<Request>(256);
    let (events_tx, events) = mpsc::unbounded_channel();

    tokio::spawn(async move {
        while let Some(request) = outbox.recv().await {
            if write_frame(&mut writer, &request).await.is_err() {
                break;
            }
        }
    });

    let waiting = pending.clone();
    tokio::spawn(async move {
        while let Ok(Some(message)) = read_frame::<_, ServerMessage>(&mut reader).await {
            match message {
                ServerMessage::Reply { id, result } => {
                    let waiter = waiting.lock().ok().and_then(|mut map| map.remove(&id));
                    if let Some(waiter) = waiter {
                        let _ = waiter.send(result);
                    }
                }
                ServerMessage::Event(event) => {
                    if events_tx.send(event).is_err() {
                        break;
                    }
                }
            }
        }
        if let Ok(mut map) = waiting.lock() {
            map.clear();
        }
    });

    Ok((Client { requests, pending, next_id: AtomicU64::new(1), server_pid }, events))
}

impl Client {
    pub async fn call(&self, op: Op) -> Result<Reply, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().map_err(|_| "client is broken".to_string())?.insert(id, tx);
        if self.requests.send(Request { id, op }).await.is_err() {
            return Err("not connected to nmux-server".into());
        }
        match rx.await {
            Ok(ReplyResult::Ok(reply)) => Ok(reply),
            Ok(ReplyResult::Err(why)) => Err(why),
            Err(_) => Err("connection to nmux-server was lost".into()),
        }
    }
}
