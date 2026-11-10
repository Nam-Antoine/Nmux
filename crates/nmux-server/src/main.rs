// No console window: the server runs in the background.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use nmux_server::{hub::Config, launch, transcripts};

fn main() -> anyhow::Result<()> {
    let mut data_dir: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data-dir" => data_dir = args.next().map(PathBuf::from),
            "--version" => {
                println!("nmux-server {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            other => anyhow::bail!("unknown argument: {other}"),
        }
    }
    let custom = data_dir.is_some();
    let data_dir = data_dir
        .or_else(nmux_server::default_data_dir)
        .ok_or_else(|| anyhow::anyhow!("cannot find a data directory; pass --data-dir"))?;

    let config = Config {
        data_dir,
        env: launch::process_env(),
        claude_dir: transcripts::claude_config_dir(),
        // Only the real per-user server takes over the Electron app's sessions.
        legacy_sessions: if custom { None } else { nmux_server::legacy_sessions_file() },
    };
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(nmux_server::run(config))
}
