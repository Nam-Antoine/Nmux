//! Saving and loading the model, and importing sessions from the Electron Nmux.

use std::path::{Path, PathBuf};

use nmux_core::{PaneKind, PaneSpec, State};
use serde::Deserialize;

const FILE: &str = "state.json";

#[derive(serde::Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    state: State,
}

pub fn state_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// Atomic: a crash never leaves a torn file.
pub fn save(data_dir: &Path, state: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let file = state_path(data_dir);
    let tmp = file.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&StoreFile { version: 1, state: state.clone() }).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &file)
}

/// `None` when there is no saved state (first run), or it cannot be read.
pub fn load(data_dir: &Path) -> Option<State> {
    let body = std::fs::read(state_path(data_dir)).ok()?;
    let file: StoreFile = match serde_json::from_slice(&body) {
        Ok(f) => f,
        Err(e) => {
            // Keep the unreadable file for inspection instead of overwriting it.
            eprintln!("[nmux] state file is not readable ({e}); starting empty");
            let _ = std::fs::rename(state_path(data_dir), data_dir.join("state.unreadable.json"));
            return None;
        }
    };
    if file.version != 1 {
        return None;
    }
    let mut state = file.state;
    state.sanitize_after_load();
    Some(state)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyFile {
    version: u32,
    sessions: Vec<LegacySession>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySession {
    name: String,
    kind: String,
    cwd: String,
    #[serde(default)]
    claude_session_id: Option<String>,
    #[serde(default)]
    claude_args: Vec<String>,
    #[serde(default)]
    skip_permissions: bool,
    #[serde(default)]
    cloud_task: Option<String>,
    #[serde(default)]
    cloud_session_id: Option<String>,
    #[serde(default)]
    cloud_url: Option<String>,
    #[serde(default)]
    teleported_from: Option<String>,
    #[serde(default)]
    created_at: u64,
}

/// Build a state from the Electron app's `sessions.json`: one workspace per
/// folder, one tab per session. Everything arrives exited; nothing is started.
pub fn import_legacy(file: &Path) -> Option<State> {
    let legacy: LegacyFile = serde_json::from_slice(&std::fs::read(file).ok()?).ok()?;
    if legacy.version != 1 {
        return None;
    }
    let mut state = State::default();
    for s in legacy.sessions {
        let kind = match s.kind.as_str() {
            "claude" => PaneKind::Claude,
            "shell" => PaneKind::Shell,
            "cloud" => PaneKind::Cloud,
            _ => continue,
        };
        let mut spec = PaneSpec::new(kind);
        spec.name = Some(s.name);
        spec.cwd = Some(s.cwd.clone());
        spec.skip_permissions = s.skip_permissions;
        spec.extra_args = s.claude_args;
        spec.cloud_task = s.cloud_task;
        spec.teleport = s.teleported_from;

        let existing = state.workspaces.iter().find(|w| same_folder(&w.cwd, &s.cwd)).map(|w| w.id);
        let created = match existing {
            Some(ws) => state.create_tab(ws, None, spec, s.created_at).ok()?,
            None => state.create_workspace(None, s.cwd, spec, s.created_at),
        };
        if let Some(pane) = state.panes.get_mut(&created.pane) {
            pane.claude_session_id = s.claude_session_id;
            pane.cloud_session_id = s.cloud_session_id;
            pane.cloud_url = s.cloud_url;
        }
    }
    state.sanitize_after_load();
    if let Some(first) = state.workspaces.first().map(|w| w.id) {
        let _ = state.select_workspace(first);
    }
    Some(state)
}

fn same_folder(a: &str, b: &str) -> bool {
    // Windows paths compare without case and with either slash, whatever the host.
    let is_drive = |p: &str| p.as_bytes().get(1) == Some(&b':');
    if is_drive(a) && is_drive(b) {
        let norm = |p: &str| p.trim_end_matches(['/', '\\']).replace('/', "\\").to_lowercase();
        norm(a) == norm(b)
    } else {
        a.trim_end_matches('/') == b.trim_end_matches('/')
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmux_core::PaneStatus;

    #[test]
    fn save_then_load() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load(dir.path()).is_none());
        let mut state = State::default();
        let c = state.create_workspace(None, "C:\\w".into(), PaneSpec::new(PaneKind::Claude), 5);
        save(dir.path(), &state).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.workspaces, state.workspaces);
        assert_eq!(loaded.panes[&c.pane].status, PaneStatus::Exited, "nothing is running after a restart");
    }

    #[test]
    fn an_unreadable_state_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(state_path(dir.path()), "{ not json").unwrap();
        assert!(load(dir.path()).is_none());
        assert!(dir.path().join("state.unreadable.json").exists());
    }

    #[test]
    fn legacy_sessions_become_workspaces_and_tabs() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sessions.json");
        std::fs::write(
            &file,
            r#"{"version":1,"sessions":[
              {"id":"a","name":"Nmux","kind":"claude","cwd":"C:\\w\\Nmux","claudeSessionId":"u1","claudeArgs":[],"skipPermissions":true,"createdAt":1,"lastActiveAt":2,"status":"running","pid":4,"working":true},
              {"id":"b","name":"logs","kind":"shell","cwd":"c:/w/nmux/","createdAt":2,"status":"exited"},
              {"id":"c","name":"Tax","kind":"claude","cwd":"C:\\w\\Tax","claudeSessionId":"u2","teleportedFrom":"session_abcdefgh","createdAt":3,"status":"exited"},
              {"id":"d","name":"x","kind":"mystery","cwd":"C:\\w\\Tax","createdAt":3,"status":"exited"}
            ]}"#,
        )
        .unwrap();
        let state = import_legacy(&file).unwrap();
        assert_eq!(state.workspaces.len(), 2);
        assert_eq!(state.workspaces[0].tabs.len(), 2);
        assert_eq!(state.workspaces[0].name, "Nmux");
        assert_eq!(state.panes.len(), 3);
        let nmux = state.panes.values().find(|p| p.name == "Nmux").unwrap();
        assert_eq!(nmux.claude_session_id.as_deref(), Some("u1"));
        assert!(nmux.skip_permissions);
        assert_eq!(nmux.status, PaneStatus::Exited);
        let tax = state.panes.values().find(|p| p.name == "Tax").unwrap();
        assert_eq!(tax.teleported_from.as_deref(), Some("session_abcdefgh"));
        assert_eq!(state.active_workspace, Some(state.workspaces[0].id));
    }
}
