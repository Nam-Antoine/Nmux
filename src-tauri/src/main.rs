//! The Nmux window. A thin shell: it finds (or starts) `nmux-server`, relays
//! requests and events between it and the UI, and owns the tray icon. All
//! logic lives in the server, so closing this window stops nothing.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use nmux_proto::{Event, Op};
use nmux_server::client::{self, Client};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::RwLock;

struct Bridge {
    client: RwLock<Option<Arc<Client>>>,
    data_dir: PathBuf,
    /// A non-default data directory (tests) is passed on to the server.
    custom_dir: bool,
}

/// The one request path from the UI: an `Op` in, its reply out. The server
/// validates everything; the shell adds nothing and trusts nothing.
#[tauri::command]
async fn call(bridge: State<'_, Arc<Bridge>>, op: serde_json::Value) -> Result<serde_json::Value, String> {
    let op: Op = serde_json::from_value(op).map_err(|e| format!("bad request: {e}"))?;
    if matches!(op, Op::Hello { .. } | Op::Shutdown) {
        return Err("not available from the window".into());
    }
    let client = bridge.client.read().await.clone().ok_or("not connected to nmux-server")?;
    let reply = client.call(op).await?;
    serde_json::to_value(reply).map_err(|e| e.to_string())
}

/// Open a cloud session in the browser. Only claude.ai links are accepted,
/// whatever the UI (or terminal output it rendered) asks for.
#[tauri::command]
fn open_cloud_url(app: AppHandle, url: String) -> Result<(), String> {
    let rest = url.strip_prefix("https://claude.ai/").ok_or("only claude.ai links can be opened")?;
    if url.len() > 2048 || rest.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("not a valid link".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn server_binary() -> Option<PathBuf> {
    let name = if cfg!(windows) { "nmux-server.exe" } else { "nmux-server" };
    let path = std::env::current_exe().ok()?.parent()?.join(name);
    path.is_file().then_some(path)
}

fn start_server(data_dir: &Path, custom_dir: bool) -> std::io::Result<()> {
    let binary = server_binary().ok_or_else(|| std::io::Error::other("nmux-server is missing next to the app"))?;
    let mut command = std::process::Command::new(binary);
    if custom_dir {
        command.arg("--data-dir").arg(data_dir);
    }
    command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // Its own group and no console: it must outlive this window.
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    command.spawn().map(drop)
}

async fn connect_or_start(bridge: &Bridge) -> Result<(Client, UnboundedReceiver<Event>), String> {
    if let Ok(connected) = client::connect(&bridge.data_dir).await {
        return Ok(connected);
    }
    start_server(&bridge.data_dir, bridge.custom_dir).map_err(|e| e.to_string())?;
    let mut last = String::new();
    for _ in 0..80 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        match client::connect(&bridge.data_dir).await {
            Ok(connected) => return Ok(connected),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("nmux-server did not start: {last}"))
}

/// Keep a connection to the server for as long as the window lives.
async fn relay(app: AppHandle, bridge: Arc<Bridge>) {
    loop {
        match connect_or_start(&bridge).await {
            Ok((client, mut events)) => {
                *bridge.client.write().await = Some(Arc::new(client));
                let _ = app.emit("nmux://connected", ());
                while let Some(event) = events.recv().await {
                    let _ = app.emit("nmux://event", &event);
                }
                *bridge.client.write().await = None;
                let _ = app.emit("nmux://disconnected", "connection to nmux-server was lost");
            }
            Err(why) => {
                let _ = app.emit("nmux://disconnected", why);
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn quit(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let bridge = app.state::<Arc<Bridge>>().inner().clone();
        let client = bridge.client.read().await.clone();
        if let Some(client) = client {
            let _ = tokio::time::timeout(Duration::from_secs(3), client.call(Op::Shutdown)).await;
        }
        app.exit(0);
    });
}

fn main() {
    let custom = std::env::var_os("NMUX_DATA_DIR").map(PathBuf::from);
    let custom_dir = custom.is_some();
    let data_dir = custom.or_else(nmux_server::default_data_dir).expect("no data directory for Nmux");
    let bridge = Arc::new(Bridge { client: RwLock::new(None), data_dir, custom_dir });

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(bridge.clone())
        .invoke_handler(tauri::generate_handler![call, open_cloud_url])
        .setup(move |app| {
            let show = MenuItem::with_id(app, "show", "Show Nmux", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit Nmux (stops every pane)", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit_item])?;
            let mut tray = TrayIconBuilder::new().tooltip("Nmux").menu(&menu).show_menu_on_left_click(false);
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.on_menu_event(|app, event| match event.id.as_ref() {
                "show" => show_main(app),
                "quit" => quit(app),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                    show_main(tray.app_handle());
                }
            })
            .build(app)?;

            tauri::async_runtime::spawn(relay(app.handle().clone(), bridge.clone()));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window only hides it; the panes keep running.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Nmux");
}
