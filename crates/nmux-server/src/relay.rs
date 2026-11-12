use std::io::{BufRead, BufReader, Error, Read, Result, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use nmux_core::PaneId;

use crate::launch::beside;
use crate::ports::quiet;
use crate::serve::{new_token, same_token};

pub const REMOTE_NC: &str = include_str!("nc.sh");

const REQUEST_LIMIT: u64 = 32 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Relay {
    port: u16,
    token: String,
}

pub struct Link {
    pub forward: String,
    pub note: String,
}

impl Relay {
    pub fn start(data_dir: &Path) -> Result<Relay> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let relay = Relay { port: listener.local_addr()?.port(), token: new_token()? };
        let (token, data_dir) = (relay.token.clone(), data_dir.to_path_buf());
        std::thread::Builder::new().name("nmux-relay".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let (token, data_dir) = (token.clone(), data_dir.clone());
                std::thread::spawn(move || answer(stream, &token, &data_dir));
            }
        })?;
        Ok(relay)
    }

    pub fn link(&self, pane: PaneId) -> Link {
        let mut random = [0u8; 2];
        let _ = getrandom::fill(&mut random);
        let far = 20000 + u16::from_le_bytes(random) % 40000;
        Link { forward: format!("127.0.0.1:{far}:127.0.0.1:{}", self.port), note: format!("{far} {} {}", self.token, pane.0) }
    }
}

fn part(reader: &mut impl BufRead) -> Result<Vec<u8>> {
    let mut size = String::new();
    reader.read_line(&mut size)?;
    let mut bytes = vec![0; size.trim().parse().map_err(Error::other)?];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn answer(stream: TcpStream, token: &str, data_dir: &PathBuf) -> Result<()> {
    stream.set_read_timeout(Some(REQUEST_TIMEOUT))?;
    let mut reader = BufReader::new(stream.try_clone()?.take(REQUEST_LIMIT));
    let mut head = String::new();
    reader.read_line(&mut head)?;
    let [given, pane, tty, input, count] = head.split_whitespace().collect::<Vec<_>>()[..] else { return Ok(()) };
    if !same_token(given, token) || pane.parse::<u64>().is_err() {
        return Ok(());
    }
    let mut command = Command::new(beside("nc").ok_or_else(|| Error::other("nc is missing"))?);
    for _ in 0..count.parse::<usize>().map_err(Error::other)? {
        command.arg(String::from_utf8_lossy(&part(&mut reader)?).into_owned());
    }
    let typed = if input == "1" { part(&mut reader)? } else { Vec::new() };
    command.env("NMUX_PANE_ID", pane).env("NMUX_DATA_DIR", data_dir);
    if tty == "1" {
        command.env("NMUX_TTY", "1");
    }
    let mut child = quiet(&mut command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(&typed);
    }
    let out = child.wait_with_output()?;
    let mut stream = stream;
    stream.write_all(format!("{} {}\n", out.status.code().unwrap_or(1), out.stderr.len()).as_bytes())?;
    stream.write_all(&out.stderr)?;
    stream.write_all(&out.stdout)
}
