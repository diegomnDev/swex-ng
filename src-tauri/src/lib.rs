mod cert;
mod decode;
mod macos;
mod mapping;
mod profile;
mod proxy;
mod settings;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Emitter, Manager, State};

#[derive(Default)]
struct AppState {
    key: Mutex<Option<[u8; 16]>>,
    shutdown: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    service: Mutex<Option<String>>,
    running: Mutex<bool>,
    /// User-chosen output folder; None = the default `app_data/profiles`.
    out_dir: Mutex<Option<PathBuf>>,
}

fn config_dir(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn profiles_dir(app: &tauri::AppHandle) -> PathBuf {
    let d = config_dir(app).join("profiles");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// Where profiles are written: the user's chosen folder if set, else the
/// default `app_data/profiles`. Ensures the folder exists.
fn current_out_dir(app: &tauri::AppHandle, state: &AppState) -> PathBuf {
    if let Some(d) = state.out_dir.lock().unwrap().clone() {
        let _ = std::fs::create_dir_all(&d);
        return d;
    }
    profiles_dir(app)
}

#[derive(serde::Serialize)]
struct Status {
    running: bool,
    key_loaded: bool,
    out_dir: String,
}

#[tauri::command]
fn get_status(app: tauri::AppHandle, state: State<AppState>) -> Status {
    Status {
        running: *state.running.lock().unwrap(),
        key_loaded: state.key.lock().unwrap().is_some(),
        out_dir: current_out_dir(&app, &state).to_string_lossy().into(),
    }
}

/// Set (and persist) the folder where captured profiles are saved.
#[tauri::command]
fn set_out_dir(app: tauri::AppHandle, state: State<AppState>, path: String) -> Result<(), String> {
    let dir = PathBuf::from(path.trim());
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't use that folder: {e}"))?;
    let _ = std::fs::create_dir_all(config_dir(&app));
    std::fs::write(
        config_dir(&app).join("out_dir.txt"),
        dir.to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    *state.out_dir.lock().unwrap() = Some(dir);
    Ok(())
}

/// Reset the output folder back to the default `app_data/profiles`.
#[tauri::command]
fn reset_out_dir(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let _ = std::fs::remove_file(config_dir(&app).join("out_dir.txt"));
    *state.out_dir.lock().unwrap() = None;
    Ok(())
}

#[tauri::command]
fn set_key(app: tauri::AppHandle, state: State<AppState>, hex: String) -> Result<(), String> {
    let key = decode::parse_key_hex(&hex).map_err(|e| e.to_string())?;
    // persist for next launches
    let path = config_dir(&app).join("key.hex");
    let _ = std::fs::create_dir_all(config_dir(&app));
    std::fs::write(path, hex.trim()).map_err(|e| e.to_string())?;
    *state.key.lock().unwrap() = Some(key);
    Ok(())
}

#[tauri::command]
async fn start_proxy(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    port: u16,
) -> Result<(), String> {
    if *state.running.lock().unwrap() {
        return Err("Proxy already running".into());
    }
    let key = state
        .key
        .lock()
        .unwrap()
        .ok_or("No decryption key set. Paste your 32-char hex key first.")?;

    // CA: generate/load + trust in the System keychain. Trust is best-effort:
    // if it's already trusted we skip the admin prompt; if the in-app prompt
    // can't show its dialog (e.g. launched from a terminal, no GUI session ->
    // "SecTrustSettingsSetTrustSettings: no user interaction possible"), we log
    // the one manual command and CONTINUE instead of aborting, so the proxy
    // still comes up and works once the cert is trusted.
    let ca = cert::load_or_create_ca(&config_dir(&app).join("ca")).map_err(|e| e.to_string())?;
    if macos::is_ca_trusted(&ca.cert_path) {
        let _ = app.emit(
            "proxy-log",
            serde_json::json!({"level":"info","message":"CA already trusted."}),
        );
    } else if let Err(e) = macos::trust_certificate(&ca.cert_path) {
        let cmd = macos::manual_trust_command(&ca.cert_path);
        let _ = app.emit("proxy-log", serde_json::json!({"level":"warning","message":
            format!("Couldn't auto-trust the CA ({e}). Run this once in Terminal, then restart the proxy:\n{cmd}")}));
    }

    // System HTTPS proxy -> us.
    let service = macos::primary_service();
    macos::set_proxy(&service, "127.0.0.1", port).map_err(|e| format!("set proxy: {e}"))?;
    *state.service.lock().unwrap() = Some(service);

    let (tx, rx) = tokio::sync::oneshot::channel();
    *state.shutdown.lock().unwrap() = Some(tx);
    *state.running.lock().unwrap() = true;

    let cfg = settings::resolve(&settings::load(&config_dir(&app)));
    let handler = proxy::SwHandler::new(key, current_out_dir(&app, &state), app.clone(), cfg);
    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = proxy::run_proxy(&ca, addr, handler, rx).await {
            let _ = app2.emit(
                "proxy-log",
                serde_json::json!({"level":"error","message": format!("proxy stopped: {e}")}),
            );
        }
    });

    let _ = app.emit("proxy-log", serde_json::json!({"level":"info","message": format!("Listening on 127.0.0.1:{port}. Open Summoners War and wait for the login screen.")}));
    Ok(())
}

#[tauri::command]
fn stop_proxy(state: State<AppState>) -> Result<(), String> {
    if let Some(tx) = state.shutdown.lock().unwrap().take() {
        let _ = tx.send(());
    }
    if let Some(service) = state.service.lock().unwrap().take() {
        let _ = macos::unset_proxy(&service);
    }
    *state.running.lock().unwrap() = false;
    Ok(())
}

/// Compute rune efficiency for a rune object (bonus helper for the optimizer).
#[tauri::command]
fn rune_efficiency(rune: serde_json::Value) -> Option<mapping::RuneEfficiency> {
    mapping::get_rune_efficiency(&rune)
}

#[tauri::command]
fn monster_name(id: i64) -> String {
    mapping::get_monster_name(id)
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> settings::Settings {
    settings::load(&config_dir(&app))
}

#[tauri::command]
fn set_settings(app: tauri::AppHandle, settings: settings::Settings) -> Result<(), String> {
    settings::save(&config_dir(&app), &settings)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .on_menu_event(|app, event| {
            // "Check for Updates…" lives in the native macOS menu; tell the
            // frontend to run its updater check.
            if event.id().0 == "check-update" {
                let _ = app.emit("menu-check-update", ());
            }
        })
        .setup(|app| {
            // Load key from env or persisted key.hex
            let handle = app.handle().clone();
            let state = handle.state::<AppState>();
            let from_env = std::env::var("SWEX_KEY").ok();
            let from_file = std::fs::read_to_string(config_dir(&handle).join("key.hex")).ok();
            if let Some(hex) = from_env.or(from_file) {
                if let Ok(k) = decode::parse_key_hex(&hex) {
                    *state.key.lock().unwrap() = Some(k);
                }
            }
            // Load a previously chosen output folder, if any.
            if let Ok(d) = std::fs::read_to_string(config_dir(&handle).join("out_dir.txt")) {
                let d = d.trim();
                if !d.is_empty() {
                    *state.out_dir.lock().unwrap() = Some(PathBuf::from(d));
                }
            }

            // Native macOS menu: app menu (About / Check for Updates / Quit) plus
            // a standard Edit menu so copy/paste works in the text fields.
            use tauri::menu::{AboutMetadata, MenuBuilder, SubmenuBuilder};
            let app_menu = SubmenuBuilder::new(app, "SWEX-NG")
                .about(Some(AboutMetadata {
                    name: Some("SWEX-NG".into()),
                    version: Some(env!("CARGO_PKG_VERSION").into()),
                    comments: Some("Native Summoners War profile exporter".into()),
                    copyright: Some("Apache-2.0 · derived from Xzandro/sw-exporter".into()),
                    website: Some("https://github.com/diegomnDev/swex-ng".into()),
                    website_label: Some("GitHub".into()),
                    authors: Some(vec!["diegomnDev".into()]),
                    ..Default::default()
                }))
                .separator()
                .text("check-update", "Check for Updates…")
                .separator()
                .quit()
                .build()?;
            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .undo()
                .redo()
                .separator()
                .cut()
                .copy()
                .paste()
                .select_all()
                .build()?;
            let menu = MenuBuilder::new(app)
                .items(&[&app_menu, &edit_menu])
                .build()?;
            app.set_menu(menu)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            set_key,
            start_proxy,
            stop_proxy,
            rune_efficiency,
            monster_name,
            set_out_dir,
            reset_out_dir,
            get_settings,
            set_settings
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Safety net: if the app quits while the proxy is on, undo the system
            // HTTPS proxy so the user isn't left with no internet (it would still
            // point at our now-dead listener). Mirrors stop_proxy's cleanup.
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = app_handle.state::<AppState>();
                // Bind out of the lock first so the MutexGuard temporaries drop
                // before `state` does (avoids E0597).
                let tx = state.shutdown.lock().unwrap().take();
                let service = state.service.lock().unwrap().take();
                if let Some(tx) = tx {
                    let _ = tx.send(());
                }
                if let Some(service) = service {
                    let _ = macos::unset_proxy(&service);
                }
            }
        });
}
