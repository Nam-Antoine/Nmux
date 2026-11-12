use std::collections::HashMap;
use std::io::{BufRead, BufReader, Error, Read, Result, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmux_core::PaneId;

use crate::launch::beside;
use crate::ports::quiet;
use crate::serve::{new_token, same_token};

pub const REMOTE_NC: &str = include_str!("nc.sh");

const REQUEST_LIMIT: u64 = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_AT_ONCE: usize = 32;

struct Secrets {
    token: String,
    proof: String,
}

type Links = Arc<Mutex<HashMap<PaneId, Secrets>>>;

pub struct Relay {
    port: u16,
    links: Links,
}

pub struct Link {
    pub forward: String,
    pub note: String,
}

impl Relay {
    pub fn start(data_dir: &Path) -> Result<Relay> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let relay = Relay { port: listener.local_addr()?.port(), links: Links::default() };
        let (links, data_dir) = (relay.links.clone(), data_dir.to_path_buf());
        std::thread::Builder::new().name("nmux-relay".into()).spawn(move || {
            let busy = Arc::new(AtomicUsize::new(0));
            for stream in listener.incoming().flatten() {
                if busy.fetch_add(1, Ordering::SeqCst) >= MAX_AT_ONCE {
                    busy.fetch_sub(1, Ordering::SeqCst);
                    continue;
                }
                let (links, data_dir, busy) = (links.clone(), data_dir.clone(), busy.clone());
                std::thread::spawn(move || {
                    let _ = answer(stream, &links, &data_dir);
                    busy.fetch_sub(1, Ordering::SeqCst);
                });
            }
        })?;
        Ok(relay)
    }

    pub fn link(&self, pane: PaneId) -> Result<Link> {
        let mut random = [0u8; 2];
        getrandom::fill(&mut random).map_err(|e| Error::other(e.to_string()))?;
        let far = 20000 + u16::from_le_bytes(random) % 40000;
        let secrets = Secrets { token: new_token()?, proof: new_token()? };
        let note = format!("{far} {} {} {}", secrets.token, pane.0, secrets.proof);
        self.links.lock().map_err(|_| Error::other("the relay is broken"))?.insert(pane, secrets);
        Ok(Link { forward: format!("127.0.0.1:{far}:127.0.0.1:{}", self.port), note })
    }

    pub fn forget(&self, pane: PaneId) {
        if let Ok(mut links) = self.links.lock() {
            links.remove(&pane);
        }
    }
}

fn line(reader: &mut impl BufRead) -> Result<String> {
    let mut text = String::new();
    reader.read_line(&mut text)?;
    Ok(text)
}

fn part(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut bytes = vec![0; line(reader)?.trim().parse().map_err(Error::other)?];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn answer(mut stream: TcpStream, links: &Links, data_dir: &PathBuf) -> Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?.take(REQUEST_LIMIT));
    let pane = PaneId(line(&mut reader)?.trim().parse().map_err(Error::other)?);
    let known = links.lock().ok().and_then(|links| links.get(&pane).map(|secrets| (secrets.token.clone(), secrets.proof.clone())));
    let Some((token, proof)) = known else { return Ok(()) };
    stream.write_all(format!("{proof}\n").as_bytes())?;
    let head = line(&mut reader)?;
    let [given, tty, input, count] = head.split_whitespace().collect::<Vec<_>>()[..] else { return Ok(()) };
    if !same_token(given, &token) {
        return Ok(());
    }
    let mut command = Command::new(beside("nc").ok_or_else(|| Error::other("nc is missing"))?);
    for _ in 0..count.parse::<usize>().map_err(Error::other)? {
        command.arg(String::from_utf8_lossy(&part(&mut reader)?).into_owned());
    }
    let typed = if input == "1" { part(&mut reader)? } else { Vec::new() };
    command.env("NMUX_PANE_ID", pane.0.to_string()).env("NMUX_DATA_DIR", data_dir).env("NMUX_RELAY", "1");
    if tty == "1" {
        command.env("NMUX_TTY", "1");
    }
    let mut child = quiet(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&typed);
    }
    let out = child.wait_with_output()?;
    stream.write_all(format!("{} {}\n", out.status.code().unwrap_or(1), out.stderr.len()).as_bytes())?;
    stream.write_all(&out.stderr)?;
    stream.write_all(&out.stdout)
}
