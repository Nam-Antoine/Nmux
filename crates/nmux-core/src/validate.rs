use crate::model::{PaneKind, PaneSpec};

pub const MAX_NAME: usize = 80;
pub const MAX_PATH: usize = 1024;
pub const MAX_ARGS: usize = 32;
pub const MAX_ARG_LEN: usize = 200;
pub const MAX_CLOUD_TASK: usize = 4000;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Rejected(pub String);

fn reject<T>(why: impl Into<String>) -> Result<T, Rejected> {
    Err(Rejected(why.into()))
}

fn has_control(text: &str, allow_newlines: bool) -> bool {
    text.chars().any(|c| c.is_control() && !(allow_newlines && (c == '\n' || c == '\r' || c == '\t')))
}

pub fn name(raw: &str) -> Result<String, Rejected> {
    let n = raw.trim();
    if n.is_empty() {
        return reject("name is empty");
    }
    if n.chars().count() > MAX_NAME {
        return reject("name is too long");
    }
    if has_control(n, false) {
        return reject("name has control characters");
    }
    Ok(n.to_string())
}

pub fn cwd(raw: &str) -> Result<String, Rejected> {
    let p = raw.trim();
    if p.is_empty() || p.len() > MAX_PATH {
        return reject("folder path is empty or too long");
    }
    if has_control(p, false) {
        return reject("folder path has control characters");
    }
    let bytes = p.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
    if !(drive || p.starts_with('/') || p.starts_with("\\\\")) {
        return reject("folder path must be absolute");
    }
    Ok(p.to_string())
}

pub fn remote_cwd(raw: &str) -> Result<String, Rejected> {
    let p = raw.trim();
    if p.is_empty() || p.len() > MAX_PATH {
        return reject("folder path is empty or too long");
    }
    if has_control(p, false) {
        return reject("folder path has control characters");
    }
    if !(p.starts_with('/') || p == "~" || p.starts_with("~/")) {
        return reject("a folder on another machine must start with / or ~");
    }
    Ok(p.to_string())
}

pub fn ssh_target(raw: &str) -> Result<String, Rejected> {
    let t = raw.trim();
    if t.is_empty() || t.len() > 255 {
        return reject("ssh target is empty or too long");
    }
    if t.starts_with('-') {
        return reject("ssh target must not start with a dash");
    }
    if !t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@' | ':' | '/' | '[' | ']' | '%')) {
        return reject("ssh target has characters that do not belong in a host name");
    }
    Ok(t.to_string())
}

pub fn extra_args(raw: &[String]) -> Result<Vec<String>, Rejected> {
    if raw.len() > MAX_ARGS {
        return reject("too many extra arguments");
    }
    for a in raw {
        if a.is_empty() || a.len() > MAX_ARG_LEN || has_control(a, false) {
            return reject("an extra argument is empty, too long, or has control characters");
        }
    }
    Ok(raw.to_vec())
}

pub fn cloud_session_id(raw: &str) -> Result<String, Rejected> {
    let id = raw.trim();
    let body = id.strip_prefix("session_").or_else(|| id.strip_prefix("cse_"));
    match body {
        Some(b) if !b.is_empty() && b.len() <= 120 && b.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') => {
            Ok(id.to_string())
        }
        _ => reject("not a cloud session id"),
    }
}

pub fn cloud_task(raw: &str) -> Result<String, Rejected> {
    let t = raw.trim();
    if t.is_empty() {
        return reject("cloud task is empty");
    }
    if t.chars().count() > MAX_CLOUD_TASK {
        return reject("cloud task is too long");
    }
    if has_control(t, true) {
        return reject("cloud task has control characters");
    }
    if t.starts_with('-') {
        return reject("cloud task cannot start with a dash");
    }
    Ok(t.to_string())
}

pub fn pane_spec(raw: &PaneSpec) -> Result<PaneSpec, Rejected> {
    let mut spec = PaneSpec::new(raw.kind);
    spec.name = match raw.name.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(n) => Some(name(n)?),
    };
    spec.cwd = raw.cwd.as_deref().map(cwd).transpose()?;
    spec.extra_args = extra_args(&raw.extra_args)?;
    spec.skip_permissions = raw.skip_permissions;

    match raw.kind {
        PaneKind::Cloud => {
            let Some(task) = raw.cloud_task.as_deref() else { return reject("a cloud pane needs a task") };
            spec.cloud_task = Some(cloud_task(task)?);
            if raw.teleport.is_some() || raw.skip_permissions {
                return reject("teleport and skip-permissions do not apply to a cloud pane");
            }
        }
        PaneKind::Claude => {
            if raw.cloud_task.is_some() {
                return reject("only a cloud pane takes a task");
            }
            spec.teleport = raw.teleport.as_deref().map(cloud_session_id).transpose()?;
        }
        PaneKind::Shell => {
            if raw.cloud_task.is_some() || raw.teleport.is_some() || raw.skip_permissions || !raw.extra_args.is_empty() {
                return reject("a shell pane takes no Claude options");
            }
        }
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(name("  api  ").unwrap(), "api");
        assert!(name("").is_err());
        assert!(name("a\nb").is_err());
        assert!(name(&"x".repeat(81)).is_err());
    }

    #[test]
    fn folders_must_be_absolute() {
        assert!(cwd("C:\\Users\\me").is_ok());
        assert!(cwd("c:/x").is_ok());
        assert!(cwd("/home/me").is_ok());
        assert!(cwd("\\\\server\\share").is_ok());
        assert!(cwd("relative\\path").is_err());
        assert!(cwd("C:relative").is_err());
        assert!(cwd("").is_err());
    }

    #[test]
    fn cloud_ids() {
        assert!(cloud_session_id("session_01PbVN3TZ73CTZQcg8YVCbii").is_ok());
        assert!(cloud_session_id("cse_abc-1").is_ok());
        assert!(cloud_session_id("session_").is_err());
        assert!(cloud_session_id("session_a b").is_err());
        assert!(cloud_session_id("--flag").is_err());
    }

    #[test]
    fn cloud_tasks() {
        assert!(cloud_task("Fix it\nthen test").is_ok());
        assert!(cloud_task("-rf").is_err());
        assert!(cloud_task("bad\x1b[31m").is_err());
        assert!(cloud_task("  ").is_err());
    }

    #[test]
    fn specs_keep_options_with_the_right_kind() {
        let mut shell = PaneSpec::new(PaneKind::Shell);
        assert!(pane_spec(&shell).is_ok());
        shell.skip_permissions = true;
        assert!(pane_spec(&shell).is_err());

        let mut cloud = PaneSpec::new(PaneKind::Cloud);
        assert!(pane_spec(&cloud).is_err(), "needs a task");
        cloud.cloud_task = Some(" do it ".into());
        assert_eq!(pane_spec(&cloud).unwrap().cloud_task.as_deref(), Some("do it"));

        let mut claude = PaneSpec::new(PaneKind::Claude);
        claude.teleport = Some("not-an-id".into());
        assert!(pane_spec(&claude).is_err());
        claude.teleport = Some("session_abc".into());
        claude.extra_args = vec!["--model".into(), "opus".into()];
        assert!(pane_spec(&claude).is_ok());
        claude.cloud_task = Some("x".into());
        assert!(pane_spec(&claude).is_err());
    }

    #[test]
    fn extra_args_are_bounded() {
        assert!(extra_args(&vec!["a".to_string(); 33]).is_err());
        assert!(extra_args(&["".to_string()]).is_err());
        assert!(extra_args(&["a\nb".to_string()]).is_err());
    }
}
