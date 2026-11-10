pub mod client;
pub mod cloud_id;
pub mod files;
pub mod git;
pub mod hub;
pub mod launch;
pub mod persist;
pub mod ports;
pub mod pty;
pub mod remote;
pub mod screen;
pub mod serve;
pub mod transcripts;

use std::path::PathBuf;
use std::sync::Arc;

pub fn default_data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("nmux-rs"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("nmux-rs"))
    }
}

pub fn legacy_sessions_file() -> Option<PathBuf> {
    let file = PathBuf::from(std::env::var_os("APPDATA")?).join("nmux").join("sessions.json");
    file.is_file().then_some(file)
}

pub async fn run(config: hub::Config) -> anyhow::Result<()> {
    let data_dir = config.data_dir.clone();
    let hub = hub::Hub::new(config);

    let ticker = Arc::downgrade(&hub);
    std::thread::Builder::new().name("nmux-tick".into()).spawn(move || {
        while let Some(hub) = ticker.upgrade() {
            hub.tick();
            drop(hub);
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    })?;

    serve::run(hub, &data_dir).await
}
