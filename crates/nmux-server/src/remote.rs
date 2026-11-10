//! Panes on another machine.
//!
//! A remote pane is the user's own `ssh` in a local pseudo-terminal. On the
//! far side the program runs inside a tmux session when tmux is there, so it
//! keeps going when the connection drops or Nmux closes, and starting the
//! pane again picks it up where it was. Keys and passwords never pass through
//! Nmux: OpenSSH asks for them in the pane, like in any terminal.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use nmux_core::{MachineStatus, PaneId};

use crate::launch::{find_on_path, Launch};

type Env = BTreeMap<String, String>;

const SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";
/// How long an unattended check or clean-up may take before it is given up on.
const BACKGROUND_TIMEOUT_SECS: u32 = 12;

/// Quote one word for a POSIX shell. Single quotes are also read the same
/// way by zsh and fish, which matter because ssh hands the command line to
/// the user's login shell.
pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What a remote pane runs.
pub enum Program<'a> {
    Shell,
    Claude {
        /// UUID Nmux owns for this conversation.
        session_id: &'a str,
        /// Continue that conversation instead of starting it.
        resume: bool,
        extra_args: &'a [String],
        skip_permissions: bool,
    },
}

/// Name of the tmux session behind a pane. The creation time keeps two
/// Nmux installs that share a host from colliding on a pane id.
pub fn tmux_session(pane: PaneId, created_at_ms: u64) -> String {
    format!("nmux-{}-{}", pane.0, created_at_ms / 1000 % 1_000_000)
}

fn cd(cwd: &str) -> String {
    // `~` only expands outside quotes, so it stays outside.
    if cwd == "~" {
        "cd".to_string()
    } else if let Some(rest) = cwd.strip_prefix("~/") {
        format!("cd ~/{}", sh_quote(rest))
    } else {
        format!("cd {}", sh_quote(cwd))
    }
}

/// The `sh` script the far side runs for a pane.
pub fn script(cwd: &str, session: &str, program: &Program) -> String {
    let run = match program {
        Program::Shell => None,
        Program::Claude { session_id, resume, extra_args, skip_permissions } => {
            let mut flags = String::new();
            if *skip_permissions {
                flags.push(' ');
                flags.push_str(SKIP_PERMISSIONS_FLAG);
            }
            for arg in extra_args.iter().filter(|a| !(*skip_permissions && *a == SKIP_PERMISSIONS_FLAG)) {
                flags.push(' ');
                flags.push_str(&sh_quote(arg));
            }
            let id = sh_quote(session_id);
            let claude = if *resume {
                // Nmux cannot see the far side's transcripts; if there is
                // nothing to resume, a new conversation beats a dead pane.
                format!("claude --resume {id}{flags} || claude{flags}")
            } else {
                format!("claude --session-id {id}{flags}")
            };
            // A login, interactive shell: that is where PATH gets `claude` on most setups.
            Some(format!("${{SHELL:-sh}} -lic {}", sh_quote(&claude)))
        }
    };
    let in_tmux = run.as_deref().map(|r| format!(" {}", sh_quote(r))).unwrap_or_default();
    let plain = run.unwrap_or_else(|| "exec ${SHELL:-sh} -l".to_string());
    format!(
        "{cd} || exit 1; \
         if command -v tmux >/dev/null 2>&1; then \
         exec tmux -u new-session -A -s {session}{in_tmux} \
         \\; set-option status off \
         \\; set-option set-titles on \
         \\; set-option set-titles-string '#{{pane_title}}'; \
         else {plain}; fi",
        cd = cd(cwd),
        session = sh_quote(session),
    )
}

fn ssh_bin(env: &Env) -> String {
    find_on_path("ssh", env).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| "ssh".into())
}

/// The local command for a remote pane: `ssh -t <target> sh -c '<script>'`.
pub fn launch(target: &str, script: &str, env: &Env) -> Launch {
    Launch {
        file: ssh_bin(env),
        args: vec![
            "-t".into(),
            // Screen updates compress well; typing is unaffected.
            "-C".into(),
            "-o".into(),
            "ServerAliveInterval=30".into(),
            "-o".into(),
            "ServerAliveCountMax=4".into(),
            // Nothing after this is an option, whatever the target looks like.
            "--".into(),
            target.into(),
            format!("sh -c {}", sh_quote(script)),
        ],
    }
}

/// An `ssh` that can never stop to ask a question.
fn unattended(target: &str, remote_command: &str, env: &Env) -> Command {
    let mut command = Command::new(ssh_bin(env));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .args(["-o", "BatchMode=yes"])
        .args(["-o", &format!("ConnectTimeout={BACKGROUND_TIMEOUT_SECS}")])
        .arg("--")
        .arg(target)
        .arg(remote_command)
        .env_clear()
        .envs(env);
    command
}

/// What a connection check found.
#[derive(Debug, PartialEq, Eq)]
pub struct Probe {
    pub status: MachineStatus,
    pub detail: Option<String>,
    pub tmux: bool,
    pub claude: bool,
}

const PROBE: &str = "echo nmux-ok; command -v tmux >/dev/null 2>&1 && echo nmux-tmux; \
                     ${SHELL:-sh} -lic 'command -v claude' >/dev/null 2>&1 && echo nmux-claude; true";

/// Read the outcome of the probe command.
pub fn read_probe(stdout: &str, stderr: &str) -> Probe {
    let has = |mark: &str| stdout.lines().any(|l| l.trim() == mark);
    if has("nmux-ok") {
        return Probe { status: MachineStatus::Online, detail: None, tmux: has("nmux-tmux"), claude: has("nmux-claude") };
    }
    let reason = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("ssh gave no reason");
    let lower = stderr.to_lowercase();
    let auth = lower.contains("permission denied") || lower.contains("authentication") || lower.contains("passphrase");
    Probe {
        status: if auth { MachineStatus::Auth } else { MachineStatus::Error },
        detail: Some(reason.chars().take(300).collect()),
        tmux: false,
        claude: false,
    }
}

/// Connect without prompting and see what is there. Blocks; call off the lock.
pub fn probe(target: &str, env: &Env) -> Probe {
    let command = format!("sh -c {}", sh_quote(PROBE));
    match unattended(target, &command, env).stdin(Stdio::null()).output() {
        Ok(out) => read_probe(&String::from_utf8_lossy(&out.stdout), &String::from_utf8_lossy(&out.stderr)),
        Err(err) => Probe {
            status: MachineStatus::Error,
            detail: Some(format!("could not run ssh: {err}")),
            tmux: false,
            claude: false,
        },
    }
}

/// End the tmux sessions of closed panes. Best effort, in the background:
/// the machine may be off, and closing a pane must not wait for it.
pub fn end_sessions(target: String, sessions: Vec<String>, env: Env) {
    if sessions.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        // `=name` is an exact match; a bare name would also match a prefix.
        let kills: Vec<String> =
            sessions.iter().map(|s| format!("tmux kill-session -t {} 2>/dev/null", sh_quote(&format!("={s}")))).collect();
        let command = format!("sh -c {}", sh_quote(&format!("{}; true", kills.join("; "))));
        let _ = unattended(&target, &command, &env).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

/// Copy a pasted image to the machine and return where it landed, so the
/// path typed into the pane exists where the agent runs.
pub fn upload(target: &str, name: &str, bytes: &[u8], env: &Env) -> Result<String, String> {
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return Err("bad file name".into());
    }
    let path = format!("/tmp/nmux-{name}");
    let command = format!("sh -c {}", sh_quote(&format!("umask 077; cat > {}", sh_quote(&path))));
    let mut child = unattended(target, &command, env)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run ssh: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(bytes).map_err(|e| format!("could not send the image: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("ssh failed");
        return Err(format!("could not copy the image to the machine: {reason}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_quotes_and_spaces() {
        assert_eq!(sh_quote("plain"), "'plain'");
        assert_eq!(sh_quote("it's a dir"), "'it'\\''s a dir'");
    }

    #[test]
    fn home_relative_folders_keep_the_tilde_unquoted() {
        assert_eq!(cd("~"), "cd");
        assert_eq!(cd("~/my proj"), "cd ~/'my proj'");
        assert_eq!(cd("/srv/app"), "cd '/srv/app'");
    }

    #[test]
    fn a_shell_pane_uses_tmux_when_it_is_there() {
        let s = script("/srv/app", "nmux-7-123", &Program::Shell);
        assert!(s.starts_with("cd '/srv/app' || exit 1; "), "{s}");
        assert!(s.contains("exec tmux -u new-session -A -s 'nmux-7-123' \\; set-option status off"), "{s}");
        assert!(s.ends_with("else exec ${SHELL:-sh} -l; fi"), "{s}");
    }

    #[test]
    fn a_claude_pane_starts_then_resumes_its_own_conversation() {
        let extra = vec!["--model".to_string(), "it's".to_string(), SKIP_PERMISSIONS_FLAG.to_string()];
        let start = Program::Claude { session_id: "uuid-1", resume: false, extra_args: &extra, skip_permissions: true };
        let s = script("~/proj", "nmux-9-1", &start);
        // Without tmux the command is run directly, quoted once.
        assert!(s.contains("else ${SHELL:-sh} -lic 'claude --session-id '\\''uuid-1'\\'' --dangerously-skip-permissions "), "{s}");
        assert_eq!(s.matches(SKIP_PERMISSIONS_FLAG).count(), 2, "once per branch, not repeated from extra args: {s}");

        let resume = Program::Claude { session_id: "uuid-1", resume: true, extra_args: &[], skip_permissions: false };
        let s = script("~/proj", "nmux-9-1", &resume);
        assert!(s.contains("claude --resume '\\''uuid-1'\\'' || claude'"), "{s}");
    }

    #[test]
    fn the_target_cannot_become_an_option() {
        let l = launch("-oProxyCommand=evil", "true", &Env::new());
        let dashes = l.args.iter().position(|a| a == "--").unwrap();
        assert_eq!(l.args[dashes + 1], "-oProxyCommand=evil");
        assert_eq!(l.args.last().unwrap(), "sh -c 'true'");
    }

    #[test]
    fn probe_results_are_read_from_markers() {
        let p = read_probe("motd line\nnmux-ok\nnmux-tmux\n", "");
        assert_eq!(p, Probe { status: MachineStatus::Online, detail: None, tmux: true, claude: false });
        let p = read_probe("", "Warning: x\nuser@host: Permission denied (publickey,password).\n");
        assert_eq!(p.status, MachineStatus::Auth);
        assert_eq!(p.detail.as_deref(), Some("user@host: Permission denied (publickey,password)."));
        let p = read_probe("", "ssh: Could not resolve hostname nope: No such host is known.\n");
        assert_eq!(p.status, MachineStatus::Error);
    }
}
