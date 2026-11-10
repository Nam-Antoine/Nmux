//! Messages between `nmux-server` and a client (the desktop window, or a test).
//!
//! Framing: a 4-byte big-endian length, then that many bytes of JSON. JSON
//! keeps the protocol readable in a log and trivial to speak from another
//! language; terminal output dominates the traffic and is text already.

use nmux_core::{Closed, Created, Dir, PaneId, PaneSpec, Side, State, TabId, WorkspaceId};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Bumped when a client and server can no longer talk to each other.
pub const PROTOCOL_VERSION: u32 = 1;

/// Largest frame either side will read. A snapshot of a full scrollback fits
/// comfortably; anything bigger is a bug or an attack.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    /// Echoed in the matching [`ServerMessage::Reply`].
    pub id: u64,
    #[serde(flatten)]
    pub op: Op,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// Must be the first request. The token is in a file only this user can read.
    #[serde(rename_all = "camelCase")]
    Hello { token: String, protocol: u32 },
    GetState,

    #[serde(rename_all = "camelCase")]
    CreateWorkspace { name: Option<String>, cwd: String, pane: PaneSpec },
    #[serde(rename_all = "camelCase")]
    CreateTab { workspace: WorkspaceId, name: Option<String>, pane: PaneSpec },
    #[serde(rename_all = "camelCase")]
    SplitPane { target: PaneId, dir: Dir, pane: PaneSpec },

    ClosePane { pane: PaneId },
    CloseTab { tab: TabId },
    CloseWorkspace { workspace: WorkspaceId },

    FocusPane { pane: PaneId },
    SelectTab { tab: TabId },
    SelectWorkspace { workspace: WorkspaceId },
    FocusSide { pane: PaneId, side: Side },
    SwapPanes { a: PaneId, b: PaneId },
    SetRatio { tab: TabId, path: Vec<u8>, ratio: f32 },
    ResizeSplit { pane: PaneId, side: Side, delta: f32 },
    ToggleZoom { pane: PaneId },

    RenamePane { pane: PaneId, name: String },
    RenameTab { tab: TabId, name: Option<String> },
    RenameWorkspace { workspace: WorkspaceId, name: String },

    /// Start an exited pane again (resume the conversation, or a new shell).
    RestartPane { pane: PaneId },
    /// Bring a cloud session into a new local Claude pane next to its launcher.
    TeleportPane { pane: PaneId },

    /// Current screen and scrollback of a pane, to paint a terminal from.
    Attach { pane: PaneId },
    Write { pane: PaneId, data: String },
    Resize { pane: PaneId, cols: u16, rows: u16 },

    /// Whether the window is on screen; decides if a finished agent counts as seen.
    SetWindowVisible { visible: bool },
    /// Stop every pane and exit.
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ServerMessage {
    Reply {
        id: u64,
        #[serde(flatten)]
        result: ReplyResult,
    },
    Event(Event),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReplyResult {
    Ok(Reply),
    Err(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Reply {
    Done,
    #[serde(rename_all = "camelCase")]
    Hello { protocol: u32, server_version: String, pid: u32 },
    State { state: State },
    Created { created: Created },
    Closed { closed: Closed },
    Attached { snapshot: Snapshot },
    Flag { value: bool },
    Pane { pane: Option<PaneId> },
}

/// What a terminal needs to show a pane as it is right now.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Escape sequences that redraw scrollback and screen.
    pub data: String,
    /// Output events with a `seq` at or below this are already in `data`.
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum Event {
    /// The whole model, sent after every change. It is small.
    State { state: State },
    Output { pane: PaneId, seq: u64, data: String },
    /// A pane's agent finished or needs an answer while out of sight.
    Attention { pane: PaneId, blocked: bool },
}

pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let body = serde_json::to_vec(message).map_err(std::io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    writer.write_all(&(body.len() as u32).to_be_bytes()).await?;
    writer.write_all(&body).await?;
    writer.flush().await
}

/// `Ok(None)` is a clean end of stream between frames.
pub async fn read_frame<R, T>(reader: &mut R) -> std::io::Result<Option<T>>
where
    R: AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut len = [0u8; 4];
    match reader.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmux_core::PaneKind;

    #[test]
    fn requests_are_flat_json() {
        let req = Request { id: 7, op: Op::Resize { pane: PaneId(3), cols: 120, rows: 40 } };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"id": 7, "op": "resize", "pane": 3, "cols": 120, "rows": 40}));
        assert_eq!(serde_json::from_value::<Request>(json).unwrap(), req);
    }

    #[test]
    fn replies_and_events_round_trip() {
        let mut state = State::default();
        state.create_workspace(None, "/w".into(), PaneSpec::new(PaneKind::Claude), 1);
        let messages = [
            ServerMessage::Reply { id: 1, result: ReplyResult::Ok(Reply::Done) },
            ServerMessage::Reply { id: 2, result: ReplyResult::Err("no such pane".into()) },
            ServerMessage::Reply { id: 3, result: ReplyResult::Ok(Reply::Flag { value: true }) },
            ServerMessage::Event(Event::Output { pane: PaneId(1), seq: 9, data: "hi\r\n".into() }),
        ];
        for m in messages {
            let text = serde_json::to_string(&m).unwrap();
            assert_eq!(serde_json::from_str::<ServerMessage>(&text).unwrap(), m, "{text}");
        }
    }

    #[tokio::test]
    async fn frames_round_trip_and_oversize_is_refused() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let req = Request {
            id: 1,
            op: Op::CreateWorkspace { name: None, cwd: "C:\\x".into(), pane: PaneSpec::new(PaneKind::Shell) },
        };
        write_frame(&mut a, &req).await.unwrap();
        assert_eq!(read_frame::<_, Request>(&mut b).await.unwrap(), Some(req));

        a.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
        assert!(read_frame::<_, Request>(&mut b).await.is_err());
        drop(a);
        let (a2, mut b2) = tokio::io::duplex(8);
        drop(a2);
        assert_eq!(read_frame::<_, Request>(&mut b2).await.unwrap(), None);
    }
}
