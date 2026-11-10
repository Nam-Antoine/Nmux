//! Read-only knowledge of where Claude Code keeps conversation transcripts,
//! so Nmux can tell whether `claude --resume <id>` has anything to resume.
//!
//! Layout (Claude Code 2.x): `<config dir>/projects/<encoded cwd>/<session id>.jsonl`
//! where the config dir is `$CLAUDE_CONFIG_DIR` or `~/.claude`, and the cwd is
//! encoded by replacing every non-alphanumeric character with `-`.

use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Transcript {
    /// At least one message: `--resume` will work.
    Resumable,
    /// The file exists but holds no messages (e.g. only a Remote Control
    /// bridge record). `--resume` fails and the id cannot be reused.
    Empty,
    /// No transcript: the conversation never got a message.
    Missing,
    /// Claude's project folder for this cwd is not where we expect; assume nothing.
    Unknown,
}

pub fn claude_config_dir() -> Option<PathBuf> {
    match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => home_dir().map(|h| h.join(".claude")),
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

pub fn encode_project_dir(cwd: &str) -> String {
    cwd.trim_end_matches(['/', '\\']).chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

pub fn transcript_path(config_dir: &Path, cwd: &str, session_id: &str) -> PathBuf {
    config_dir.join("projects").join(encode_project_dir(cwd)).join(format!("{session_id}.jsonl"))
}

const MARKERS: [&[u8]; 2] = [b"\"type\":\"user\"", b"\"type\":\"assistant\""];
const CHUNK: usize = 64 * 1024;
/// Stop scanning after this much; a message shows up early if there is one.
const MAX_SCAN: usize = 8 * 1024 * 1024;

pub fn state(config_dir: &Path, cwd: &str, session_id: &str) -> Transcript {
    let file = transcript_path(config_dir, cwd, session_id);
    let Some(project) = file.parent() else { return Transcript::Unknown };
    if !project.is_dir() {
        return Transcript::Unknown;
    }
    match std::fs::metadata(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Transcript::Missing,
        Err(_) => Transcript::Unknown,
        Ok(m) if m.len() == 0 => Transcript::Empty,
        Ok(_) => match has_messages(&file) {
            Ok(true) => Transcript::Resumable,
            Ok(false) => Transcript::Empty,
            Err(_) => Transcript::Unknown,
        },
    }
}

fn has_messages(file: &Path) -> std::io::Result<bool> {
    let mut f = std::fs::File::open(file)?;
    let mut buf = vec![0u8; CHUNK];
    // Keep a tail so a marker split across chunks is still found.
    let mut window: Vec<u8> = Vec::new();
    let mut scanned = 0;
    while scanned < MAX_SCAN {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        scanned += n;
        window.extend_from_slice(&buf[..n]);
        if MARKERS.iter().any(|m| window.windows(m.len()).any(|w| w == *m)) {
            return Ok(true);
        }
        let keep = window.len().min(32);
        window.drain(..window.len() - keep);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_dirs_are_encoded_like_claude_does() {
        assert_eq!(encode_project_dir("C:\\Users\\me\\My App\\"), "C--Users-me-My-App");
        assert_eq!(encode_project_dir("/home/me/app"), "-home-me-app");
    }

    #[test]
    fn states() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = "C:\\proj";
        assert_eq!(state(dir.path(), cwd, "a"), Transcript::Unknown);

        let project = dir.path().join("projects").join(encode_project_dir(cwd));
        std::fs::create_dir_all(&project).unwrap();
        assert_eq!(state(dir.path(), cwd, "a"), Transcript::Missing);

        std::fs::write(project.join("a.jsonl"), "").unwrap();
        assert_eq!(state(dir.path(), cwd, "a"), Transcript::Empty);

        std::fs::write(project.join("a.jsonl"), "{\"type\":\"bridge-session\"}\n").unwrap();
        assert_eq!(state(dir.path(), cwd, "a"), Transcript::Empty);

        // A marker that straddles the chunk boundary is still found.
        let mut body = vec![b' '; CHUNK - 6];
        body.extend_from_slice(b"{\"type\":\"user\",\"x\":1}\n");
        std::fs::write(project.join("a.jsonl"), body).unwrap();
        assert_eq!(state(dir.path(), cwd, "a"), Transcript::Resumable);
    }
}
