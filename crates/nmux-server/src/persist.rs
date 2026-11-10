use std::path::{Path, PathBuf};

use nmux_core::State;
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

pub fn save(data_dir: &Path, state: &State) -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir)?;
    let file = state_path(data_dir);
    let tmp = file.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(&StoreFile { version: 1, state: state.clone() }).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &file)
}

pub fn load(data_dir: &Path) -> Option<State> {
    let body = std::fs::read(state_path(data_dir)).ok()?;
    let file: StoreFile = match serde_json::from_slice(&body) {
        Ok(f) => f,
        Err(e) => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use nmux_core::{PaneKind, PaneSpec, PaneStatus};

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
}
