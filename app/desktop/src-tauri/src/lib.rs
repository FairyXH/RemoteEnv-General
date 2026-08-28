use remote_env_core::collector::CollectorEvent;

use remote_env_core::runtime::{RuntimeError, RuntimeStatus, RuntimeSupervisor};
use remote_env_core::state::StateStore;
use remote_env_platform_windows::wifi::{NativeWlanProvider, WiFiCollector, WlanProvider};
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State};

pub struct AppState(Mutex<Option<RuntimeSupervisor>>);

#[tauri::command]
fn scan_wifi() -> Result<remote_env_core::collector::CollectorEvent, String> {
    WiFiCollector::new(NativeWlanProvider::new(), true)
        .scan_once()
        .map_err(|error| error.to_string())
}

fn state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("state.sqlite3"))
}

#[tauri::command]
fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "application state poisoned".to_string())?;
    Ok(guard
        .as_ref()
        .map(RuntimeSupervisor::status)
        .unwrap_or_default())
}

#[tauri::command]
fn submit_test_event(state: State<'_, AppState>, data_type: String) -> Result<(), String> {
    let guard = state
        .0
        .lock()
        .map_err(|_| "application state poisoned".to_string())?;
    let runtime = guard
        .as_ref()
        .ok_or_else(|| "runtime is not started".to_string())?;
    runtime
        .submit(CollectorEvent {
            data_type,
            timestamp_ms: 0,
            data: serde_json::json!({"mock": true}),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn start_runtime(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "application state poisoned".to_string())?;
    if guard.is_some() {
        return Ok(());
    }
    let path = state_path(&app)?;
    let store = StateStore::open(path).map_err(|e| e.to_string())?;
    let mut config = store
        .load_config()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if config.identity.device_id.is_empty() {
        config.identity = store
            .load_or_create_identity("RemoteEnvCollector", "windows", "unknown")
            .map_err(|e| e.to_string())?;
    }
    if config.token.is_empty() && config.server_profiles.is_empty() {
        return Err("configure server URL and token before starting runtime".into());
    }
    *guard = Some(
        RuntimeSupervisor::start_with_collector(
            config,
            store,
            Some(std::sync::Arc::new(|| {
                NativeWlanProvider::new()
                    .scan()
                    .and_then(|snapshot| {
                        Ok(remote_env_core::collector::CollectorEvent {
                            data_type: "wifi".into(),
                            timestamp_ms: 0,
                            data: serde_json::to_value(snapshot).map_err(|error| {
                                remote_env_platform_windows::wifi::WiFiError::InvalidData(
                                    error.to_string(),
                                )
                            })?,
                        })
                    })
                    .map_err(|error| error.to_string())
            })),
        )
        .map_err(|e: RuntimeError| e.to_string())?,
    );
    Ok(())
}

#[tauri::command]
fn stop_runtime(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .0
        .lock()
        .map_err(|_| "application state poisoned".to_string())?;
    if let Some(mut runtime) = guard.take() {
        runtime.stop();
    }
    Ok(())
}

pub fn run() {
    tauri::Builder::default()
        .manage(AppState(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            scan_wifi,
            submit_test_event,
            start_runtime,
            stop_runtime
        ])
        .build(tauri::generate_context!())
        .expect("Tauri application failed")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, .. } = event {
                api.prevent_exit();
                let state = app.state::<AppState>();
                if let Ok(mut guard) = state.0.lock() {
                    if let Some(mut runtime) = guard.take() {
                        runtime.stop();
                    }
                }
                app.exit(0);
            }
        });
}
