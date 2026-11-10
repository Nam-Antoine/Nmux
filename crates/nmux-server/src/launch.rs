use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nmux_core::PaneId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub file: String,
    pub args: Vec<String>,
}

type Env = BTreeMap<String, String>;

fn env_get<'a>(env: &'a Env, key: &str) -> Option<&'a str> {
    env.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
}

pub fn find_on_path(name: &str, env: &Env) -> Option<PathBuf> {
    let path = env_get(env, "PATH")?;
    let exts: Vec<String> = if cfg!(windows) {
        env_get(env, "PATHEXT").unwrap_or(".EXE;.CMD;.BAT;.COM").split(';').map(str::to_lowercase).collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(path) {
        for ext in &exts {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn claude_bin(env: &Env) -> String {
    find_on_path("claude", env)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| if cfg!(windows) { "claude.exe".into() } else { "claude".into() })
}

fn is_cmd_shim(bin: &str) -> bool {
    let lower = bin.to_lowercase();
    cfg!(windows) && (lower.ends_with(".cmd") || lower.ends_with(".bat"))
}

fn claude_binary(bin: String, args: Vec<String>, env: &Env) -> Launch {
    if is_cmd_shim(&bin) {
        let comspec = env_get(env, "COMSPEC").unwrap_or("cmd.exe").to_string();
        let mut all = vec!["/d".to_string(), "/c".to_string(), bin];
        all.extend(args);
        return Launch { file: comspec, args: all };
    }
    Launch { file: bin, args }
}

const SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";

pub struct ClaudeOptions<'a> {
    pub session_id: &'a str,
    pub resume: bool,
    pub extra_args: &'a [String],
    pub skip_permissions: bool,
    pub teleport: Option<&'a str>,
}

pub fn claude(opts: ClaudeOptions, env: &Env) -> Launch {
    let mut args: Vec<String> = Vec::new();
    if opts.resume {
        args.extend(["--resume".to_string(), opts.session_id.to_string()]);
    } else {
        if let Some(id) = opts.teleport {
            args.extend(["--teleport".to_string(), id.to_string()]);
        }
        args.extend(["--session-id".to_string(), opts.session_id.to_string()]);
    }
    if opts.skip_permissions {
        args.push(SKIP_PERMISSIONS_FLAG.to_string());
    }
    args.extend(opts.extra_args.iter().filter(|a| !(opts.skip_permissions && *a == SKIP_PERMISSIONS_FLAG)).cloned());
    claude_binary(claude_bin(env), args, env)
}

pub fn cloud(task: &str, extra_args: &[String], env: &Env) -> Launch {
    let bin = claude_bin(env);
    let task: String = if is_cmd_shim(&bin) {
        task.chars().map(|c| if matches!(c, '"' | '%' | '\r' | '\n') { ' ' } else { c }).collect()
    } else {
        task.to_string()
    };
    let mut args = vec!["--cloud".to_string(), task];
    args.extend(extra_args.iter().cloned());
    claude_binary(bin, args, env)
}

pub fn shell(env: &Env) -> Launch {
    if cfg!(windows) {
        let file = find_on_path("pwsh", env)
            .or_else(|| find_on_path("powershell", env))
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "powershell.exe".into());
        Launch { file, args: vec!["-NoLogo".into()] }
    } else {
        Launch { file: env_get(env, "SHELL").unwrap_or("/bin/bash").to_string(), args: vec!["-l".into()] }
    }
}

const NESTED_CLAUDE_MARKERS: &[&str] = &[
    "CLAUDECODE",
    "CLAUDE_PID",
    "CLAUDE_EFFORT",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
];

pub fn pane_env(base: &Env, pane: PaneId) -> Env {
    let mut env: Env = base
        .iter()
        .filter(|(k, _)| !NESTED_CLAUDE_MARKERS.contains(&k.as_str()) && !k.starts_with("ELECTRON_"))
        .filter(|(k, _)| !k.starts_with("NMUX_"))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    env.insert("TERM".into(), "xterm-256color".into());
    env.insert("COLORTERM".into(), "truecolor".into());
    env.insert("TERM_PROGRAM".into(), "nmux".into());
    env.insert("NMUX_PANE_ID".into(), pane.0.to_string());
    env
}

pub fn process_env() -> Env {
    std::env::vars().collect()
}

pub fn is_dir(path: &str) -> bool {
    Path::new(path).is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env::new()
    }

    fn opts<'a>(resume: bool, teleport: Option<&'a str>, extra: &'a [String], skip: bool) -> ClaudeOptions<'a> {
        ClaudeOptions { session_id: "uuid-1", resume, extra_args: extra, skip_permissions: skip, teleport }
    }

    #[test]
    fn new_resume_and_teleport() {
        assert_eq!(claude(opts(false, None, &[], false), &env()).args, ["--session-id", "uuid-1"]);
        assert_eq!(claude(opts(true, Some("session_x"), &[], false), &env()).args, ["--resume", "uuid-1"]);
        assert_eq!(
            claude(opts(false, Some("session_x"), &[], false), &env()).args,
            ["--teleport", "session_x", "--session-id", "uuid-1"]
        );
    }

    #[test]
    fn skip_permissions_is_added_once() {
        let extra = vec![SKIP_PERMISSIONS_FLAG.to_string(), "--model".to_string(), "opus".to_string()];
        let args = claude(opts(false, None, &extra, true), &env()).args;
        assert_eq!(args, ["--session-id", "uuid-1", SKIP_PERMISSIONS_FLAG, "--model", "opus"]);
        let args = claude(opts(false, None, &extra, false), &env()).args;
        assert_eq!(args.iter().filter(|a| *a == SKIP_PERMISSIONS_FLAG).count(), 1, "kept when the user typed it");
    }

    #[test]
    fn cloud_task_is_one_argument() {
        let l = cloud("Fix \"it\"\nplease", &["--model".to_string()], &env());
        assert_eq!(l.args[0], "--cloud");
        assert_eq!(l.args[1], "Fix \"it\"\nplease");
        assert_eq!(l.args[2], "--model");
    }

    #[test]
    fn nested_claude_markers_are_stripped() {
        let mut base = Env::new();
        base.insert("CLAUDECODE".into(), "1".into());
        base.insert("ANTHROPIC_API_KEY".into(), "k".into());
        base.insert("NMUX_SESSION_ID".into(), "old".into());
        base.insert("ELECTRON_RUN_AS_NODE".into(), "1".into());
        let env = pane_env(&base, PaneId(5));
        assert!(!env.contains_key("CLAUDECODE") && !env.contains_key("NMUX_SESSION_ID"));
        assert!(!env.contains_key("ELECTRON_RUN_AS_NODE"));
        assert_eq!(env["ANTHROPIC_API_KEY"], "k");
        assert_eq!(env["NMUX_PANE_ID"], "5");
        assert_eq!(env["TERM"], "xterm-256color");
    }
}
