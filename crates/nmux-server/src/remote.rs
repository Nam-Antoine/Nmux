use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use nmux_core::{MachineStatus, PaneId};

use crate::launch::{find_on_path, Launch};

type Env = BTreeMap<String, String>;

const SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";
const BACKGROUND_TIMEOUT_SECS: u32 = 12;

pub fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

pub enum Program<'a> {
    Shell,
    Claude {
        session_id: &'a str,
        resume: bool,
        extra_args: &'a [String],
        skip_permissions: bool,
    },
}

pub fn owner(env: &Env) -> String {
    let name = env.iter().find(|(k, _)| k.eq_ignore_ascii_case("USERNAME") || k.as_str() == "USER").map(|(_, v)| v.as_str()).unwrap_or("user");
    let clean: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).take(16).collect::<String>().to_lowercase();
    if clean.is_empty() { "user".into() } else { clean }
}

pub fn tmux_session(owner: &str, pane: PaneId, created_at_ms: u64) -> String {
    format!("nmux-{owner}-{}-{}", pane.0, created_at_ms / 1000 % 1_000_000)
}

pub(crate) fn cd(cwd: &str) -> String {
    if cwd == "~" {
        "cd".to_string()
    } else if let Some(rest) = cwd.strip_prefix("~/") {
        format!("cd ~/{}", sh_quote(rest))
    } else {
        format!("cd {}", sh_quote(cwd))
    }
}

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
                format!("claude --resume {id}{flags} || claude{flags}")
            } else {
                format!("claude --session-id {id}{flags}")
            };
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

pub fn launch(target: &str, script: &str, env: &Env) -> Launch {
    Launch {
        file: ssh_bin(env),
        args: vec![
            "-t".into(),
            "-C".into(),
            "-o".into(),
            "ServerAliveInterval=30".into(),
            "-o".into(),
            "ServerAliveCountMax=4".into(),
            "--".into(),
            target.into(),
            format!("sh -c {}", sh_quote(script)),
        ],
    }
}

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

pub(crate) fn last_line(stderr: &[u8], fallback: &str) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or(fallback).chars().take(300).collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Probe {
    pub status: MachineStatus,
    pub detail: Option<String>,
    pub tmux: bool,
    pub claude: bool,
}

const PROBE: &str = "echo nmux-ok; command -v tmux >/dev/null 2>&1 && echo nmux-tmux; \
                     ${SHELL:-sh} -lic 'command -v claude' >/dev/null 2>&1 && echo nmux-claude; true";

pub fn read_probe(stdout: &str, stderr: &str) -> Probe {
    let has = |mark: &str| stdout.lines().any(|l| l.trim() == mark);
    if has("nmux-ok") {
        return Probe { status: MachineStatus::Online, detail: None, tmux: has("nmux-tmux"), claude: has("nmux-claude") };
    }
    let lower = stderr.to_lowercase();
    let auth = lower.contains("permission denied") || lower.contains("authentication") || lower.contains("passphrase");
    Probe {
        status: if auth { MachineStatus::Auth } else { MachineStatus::Error },
        detail: Some(last_line(stderr.as_bytes(), "ssh gave no reason")),
        tmux: false,
        claude: false,
    }
}

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

fn process_name(line: &str) -> String {
    line.split("((\"").nth(1).and_then(|rest| rest.split('"').next()).unwrap_or_default().to_string()
}

pub fn read_listeners(text: &str) -> Vec<(String, u16, String)> {
    let mut found: Vec<(String, u16, String)> = Vec::new();
    for line in text.lines() {
        let Some((address, port, _)) = crate::ports::parse_ss(line).into_iter().next() else { continue };
        if !found.iter().any(|f| f.1 == port) {
            found.push((address, port, process_name(line)));
        }
    }
    found.sort_by_key(|f| f.1);
    found
}

pub fn output(target: &str, script: &str, env: &Env) -> Result<std::process::Output, String> {
    let command = format!("sh -c {}", sh_quote(script));
    unattended(target, &command, env).stdin(Stdio::null()).output().map_err(|e| format!("could not run ssh: {e}"))
}

pub fn listening(target: &str, env: &Env) -> Result<Vec<(String, u16, String)>, String> {
    let out = output(target, "ss -Hltnp 2>/dev/null || netstat -ltn 2>/dev/null; echo nmux-ok", env)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    if !stdout.lines().any(|l| l.trim() == "nmux-ok") {
        return Err(last_line(&out.stderr, "ssh gave no reason"));
    }
    Ok(read_listeners(&stdout))
}

pub fn end_sessions(target: String, sessions: Vec<String>, env: Env) {
    if sessions.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let kills: Vec<String> =
            sessions.iter().map(|s| format!("tmux kill-session -t {} 2>/dev/null", sh_quote(&format!("={s}")))).collect();
        let command = format!("sh -c {}", sh_quote(&format!("{}; true", kills.join("; "))));
        let _ = unattended(&target, &command, &env).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    });
}

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
        return Err(format!("could not copy the image to the machine: {}", last_line(&out.stderr, "ssh failed")));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_carry_the_name_of_who_started_them() {
        let env: Env = [("USERNAME".to_string(), "Nam Tran!".to_string())].into();
        assert_eq!(owner(&env), "namtran");
        assert_eq!(owner(&Env::new()), "user");
        assert_eq!(tmux_session("namtran", PaneId(7), 1_794_307_748_733), "nmux-namtran-7-307748");
    }

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
    fn remote_listeners_are_read_from_ss_and_netstat() {
        let ss = "LISTEN 0 511 127.0.0.1:5173 0.0.0.0:* users:((\"node\",pid=1234,fd=20))\n\
                  LISTEN 0 128 0.0.0.0:22 0.0.0.0:*\n\
                  LISTEN 0 128 [::]:22 [::]:*\nnmux-ok\n";
        assert_eq!(
            read_listeners(ss),
            vec![("0.0.0.0".to_string(), 22, String::new()), ("127.0.0.1".to_string(), 5173, "node".to_string())]
        );
        let netstat = "Active Internet connections (only servers)\n\
                       Proto Recv-Q Send-Q Local Address           Foreign Address         State\n\
                       tcp        0      0 0.0.0.0:8080            0.0.0.0:*               LISTEN\n";
        assert_eq!(read_listeners(netstat), vec![("0.0.0.0".to_string(), 8080, String::new())]);
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
