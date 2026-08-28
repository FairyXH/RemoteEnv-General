use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, ServerMode, ServerProfile};
use remote_env_core::protocol::{AuthFrame, HeartbeatFrame};
use remote_env_core::runtime::{RuntimeStatus, RuntimeSupervisor};
use remote_env_core::state::StateStore;
use remote_env_platform_windows::bluetooth::{
    BluetoothCollector, NativeBleScanner, NativeClassicBluetoothScanner,
};
use remote_env_platform_windows::wifi::{NativeWlanProvider, WlanProvider};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tauri::{
    Emitter, Manager, State,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const STATUS_EVENT: &str = "runtime_status_changed";

pub struct AppState {
    runtime: Mutex<Option<RuntimeSupervisor>>,
    state_path: PathBuf,
    exiting: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
struct ServerProfileView {
    id: String,
    name: String,
    url: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopConfigView {
    device_id: String,
    server_mode: ServerMode,
    active_server_id: Option<String>,
    server_profiles: Vec<ServerProfileView>,
    wifi_enabled: bool,
    bluetooth_enabled: bool,
    scan_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerProfileInput {
    id: Option<String>,
    name: String,
    url: String,
    token: Option<String>,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ConnectionTestResult {
    success: bool,
    message: String,
}

fn user_error(error: impl std::fmt::Display) -> String {
    let message = error.to_string();
    if message.contains("token") || message.contains("auth") {
        "认证失败，请检查服务器地址、设备 ID 和令牌。".into()
    } else {
        "操作未完成，请检查配置、网络连接和日志后重试。".into()
    }
}

fn state_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(user_error)?;
    std::fs::create_dir_all(&dir).map_err(user_error)?;
    Ok(dir.join("state.sqlite3"))
}

fn open_store(path: &PathBuf) -> Result<StateStore, String> {
    StateStore::open(path).map_err(user_error)
}

fn load_config(path: &PathBuf) -> Result<(StateStore, ClientConfig), String> {
    let store = open_store(path)?;
    let mut config = store.load_config().map_err(user_error)?.unwrap_or_default();
    for profile in &mut config.server_profiles {
        profile.token = protect::decrypt(&profile.token).map_err(user_error)?;
    }
    if config.identity.device_id.is_empty() {
        config.identity = store
            .load_or_create_identity("RemoteEnvCollector", "windows", "")
            .map_err(user_error)?;
        save_config(&store, &config)?;
    }
    Ok((store, config))
}

fn save_config(store: &StateStore, config: &ClientConfig) -> Result<(), String> {
    let mut stored = config.clone();
    for profile in &mut stored.server_profiles {
        profile.token = protect::encrypt(&profile.token).map_err(user_error)?;
    }
    store.save_config(&stored).map_err(user_error)
}

#[cfg(windows)]
mod protect {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    use windows_sys::Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData, CryptUnprotectData},
    };

    const PREFIX: &str = "dpapi:v1:";

    pub fn encrypt(value: &str) -> Result<String, String> {
        if value.is_empty() || value.starts_with(PREFIX) {
            return Ok(value.to_string());
        }
        let input = CRYPT_INTEGER_BLOB {
            cbData: value.len() as u32,
            pbData: value.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("无法保护服务器令牌。".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
        let encoded = format!("{PREFIX}{}", STANDARD.encode(bytes));
        unsafe {
            LocalFree(output.pbData as HLOCAL);
        }
        Ok(encoded)
    }

    pub fn decrypt(value: &str) -> Result<String, String> {
        if value.is_empty() || !value.starts_with(PREFIX) {
            return Ok(value.to_string());
        }
        let raw = STANDARD
            .decode(&value[PREFIX.len()..])
            .map_err(|_| "服务器令牌存储内容无效。".to_string())?;
        let input = CRYPT_INTEGER_BLOB {
            cbData: raw.len() as u32,
            pbData: raw.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
                &mut output,
            )
        };
        if ok == 0 {
            return Err("无法读取服务器令牌，请使用当前 Windows 用户重新配置。".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
        let decoded =
            String::from_utf8(bytes.to_vec()).map_err(|_| "服务器令牌编码无效。".to_string());
        unsafe {
            LocalFree(output.pbData as HLOCAL);
        }
        decoded
    }
}

#[cfg(not(windows))]
mod protect {
    pub fn encrypt(value: &str) -> Result<String, String> {
        Ok(value.to_string())
    }
    pub fn decrypt(value: &str) -> Result<String, String> {
        Ok(value.to_string())
    }
}

fn config_view(config: &ClientConfig) -> DesktopConfigView {
    DesktopConfigView {
        device_id: config.identity.device_id.clone(),
        server_mode: config.server_mode,
        active_server_id: config.active_server_id.clone(),
        server_profiles: config
            .server_profiles
            .iter()
            .map(|profile| ServerProfileView {
                id: profile.id.clone(),
                name: profile.name.clone(),
                url: profile.url.clone(),
                enabled: profile.enabled,
            })
            .collect(),
        wifi_enabled: config.wifi_enabled,
        bluetooth_enabled: config.bluetooth_enabled,
        scan_interval_seconds: config.scan_interval_seconds,
    }
}

fn update_runtime(state: &AppState, config: &ClientConfig) -> Result<(), String> {
    let guard = state
        .runtime
        .lock()
        .map_err(|_| "应用状态不可用。".to_string())?;
    if let Some(runtime) = guard.as_ref() {
        runtime.update_config(config.clone()).map_err(user_error)?;
    }
    Ok(())
}

fn current_status(state: &AppState) -> Result<RuntimeStatus, String> {
    let guard = state
        .runtime
        .lock()
        .map_err(|_| "应用状态不可用。".to_string())?;
    Ok(guard
        .as_ref()
        .map(RuntimeSupervisor::status)
        .unwrap_or_else(|| {
            let mut status = RuntimeStatus::default();
            status.connection = remote_env_core::transport::ConnectionState::Stopped;
            status
        }))
}

#[tauri::command]
fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    current_status(&state)
}

#[tauri::command]
fn get_desktop_config(state: State<'_, AppState>) -> Result<DesktopConfigView, String> {
    let (_, config) = load_config(&state.state_path)?;
    Ok(config_view(&config))
}

#[tauri::command]
fn save_server_profile(
    input: ServerProfileInput,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    let (store, mut config) = load_config(&state.state_path)?;
    let name = input.name.trim();
    let url = input.url.trim();
    if name.is_empty() || url.is_empty() {
        return Err("服务器名称和 WebSocket 地址不能为空。".into());
    }
    if !(url.starts_with("ws://") || url.starts_with("wss://")) {
        return Err("服务器地址必须以 ws:// 或 wss:// 开头。".into());
    }

    let id = input.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Some(profile) = config
        .server_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    {
        profile.name = name.into();
        profile.url = url.into();
        profile.enabled = input.enabled;
        if let Some(token) = input.token.filter(|token| !token.is_empty()) {
            profile.token = token;
        }
    } else {
        let token = input
            .token
            .filter(|token| !token.is_empty())
            .ok_or_else(|| "新增服务器需要输入令牌。".to_string())?;
        config.server_profiles.push(ServerProfile {
            id: id.clone(),
            name: name.into(),
            url: url.into(),
            token,
            enabled: input.enabled,
        });
    }
    if config.active_server_id.is_none() {
        config.active_server_id = Some(id);
    }
    config.server_url.clear();
    config.token.clear();
    config.validate().map_err(user_error)?;
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    Ok(config_view(&config))
}

#[tauri::command]
fn delete_server_profile(
    id: String,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    let (store, mut config) = load_config(&state.state_path)?;
    config.server_profiles.retain(|profile| profile.id != id);
    if config.active_server_id.as_deref() == Some(&id) {
        config.active_server_id = config
            .server_profiles
            .first()
            .map(|profile| profile.id.clone());
    }
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    Ok(config_view(&config))
}

#[tauri::command]
fn set_runtime_options(
    server_mode: ServerMode,
    active_server_id: Option<String>,
    wifi_enabled: bool,
    bluetooth_enabled: bool,
    scan_interval_seconds: u64,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    if scan_interval_seconds == 0 || scan_interval_seconds > 3600 {
        return Err("扫描间隔必须在 1 到 3600 秒之间。".into());
    }
    let (store, mut config) = load_config(&state.state_path)?;
    config.server_mode = server_mode;
    config.active_server_id = active_server_id;
    config.wifi_enabled = wifi_enabled;
    config.bluetooth_enabled = bluetooth_enabled;
    config.scan_interval_seconds = scan_interval_seconds;
    if config.server_mode == ServerMode::Single && !config.server_profiles.is_empty() {
        let active = config.active_server_id.as_deref();
        if active.is_none()
            || !config
                .server_profiles
                .iter()
                .any(|profile| Some(profile.id.as_str()) == active)
        {
            return Err("单服务器模式必须选择有效的活动服务器。".into());
        }
    }
    config.validate().map_err(user_error)?;
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    Ok(config_view(&config))
}

#[tauri::command]
async fn test_server_profile(
    id: String,
    state: State<'_, AppState>,
) -> Result<ConnectionTestResult, String> {
    let (_, config) = load_config(&state.state_path)?;
    let profile = config
        .server_profiles
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| "未找到服务器配置。".to_string())?;
    let identity = config.identity;
    let test = async move {
        let (mut socket, _) = connect_async(&profile.url)
            .await
            .map_err(|_| "无法建立 WebSocket 连接。".to_string())?;
        let auth = AuthFrame::collector(
            profile.token,
            &identity,
            vec!["wifi".into(), "bluetooth".into()],
        );
        socket
            .send(Message::Text(
                serde_json::to_string(&auth)
                    .map_err(|_| "认证请求无效。".to_string())?
                    .into(),
            ))
            .await
            .map_err(|_| "无法发送认证请求。".to_string())?;
        let mut authenticated = false;
        let mut device_list = false;
        while !(authenticated && device_list) {
            let frame = socket
                .next()
                .await
                .ok_or_else(|| "服务器在认证完成前关闭连接。".to_string())?
                .map_err(|_| "读取服务器响应失败。".to_string())?;
            let Message::Text(text) = frame else {
                continue;
            };
            let value: serde_json::Value = serde_json::from_str(&text)
                .map_err(|_| "服务器返回了无法识别的响应。".to_string())?;
            match value["type"].as_str() {
                Some("auth_result") if value["success"].as_bool() == Some(true) => {
                    authenticated = true
                }
                Some("device_list") => device_list = true,
                Some("error") => return Err("服务器拒绝了认证请求。".into()),
                _ => {}
            }
        }
        let heartbeat = HeartbeatFrame {
            r#type: "heartbeat".into(),
            timestamp: 0,
        };
        socket
            .send(Message::Text(
                serde_json::to_string(&heartbeat)
                    .map_err(|_| "心跳请求无效。".to_string())?
                    .into(),
            ))
            .await
            .map_err(|_| "无法发送心跳请求。".to_string())?;
        while let Some(frame) = socket.next().await {
            let Message::Text(text) = frame.map_err(|_| "读取心跳响应失败。".to_string())?
            else {
                continue;
            };
            if serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|value| value["type"].as_str().map(str::to_owned))
                .as_deref()
                == Some("pong")
            {
                return Ok(ConnectionTestResult {
                    success: true,
                    message: "连接成功，认证成功，服务器可用。".into(),
                });
            }
        }
        Err("服务器未返回心跳响应。".into())
    };
    tokio::time::timeout(Duration::from_secs(15), test)
        .await
        .map_err(|_| "连接测试超时，请检查网络和服务器地址。".to_string())?
}

#[tauri::command]
fn start_runtime(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let (store, config) = load_config(&state.state_path)?;
    if config.selected_servers().is_empty() {
        return Err("请先新增并启用至少一个服务器配置。".into());
    }
    let mut guard = state
        .runtime
        .lock()
        .map_err(|_| "应用状态不可用。".to_string())?;
    if guard.is_none() {
        *guard = Some(
            RuntimeSupervisor::start_with_collectors(
                config,
                store,
                Some(std::sync::Arc::new(|| {
                    NativeWlanProvider::new()
                        .scan()
                        .and_then(|snapshot| {
                            Ok(CollectorEvent {
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
                Some(std::sync::Arc::new(|| {
                    BluetoothCollector::new(
                        NativeBleScanner::new(),
                        NativeClassicBluetoothScanner::new(),
                    )
                    .scan_once()
                    .map_err(|error| error.to_string())
                })),
            )
            .map_err(user_error)?,
        );
    }
    let status = guard
        .as_ref()
        .map(RuntimeSupervisor::status)
        .unwrap_or_default();
    let _ = app.emit(STATUS_EVENT, &status);
    Ok(status)
}

#[tauri::command]
fn stop_runtime(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let mut guard = state
        .runtime
        .lock()
        .map_err(|_| "应用状态不可用。".to_string())?;
    if let Some(mut runtime) = guard.take() {
        runtime.stop();
    }
    let status = RuntimeStatus::default();
    let _ = app.emit(STATUS_EVENT, &status);
    Ok(status)
}

fn spawn_status_bridge(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(200));
            if app.state::<AppState>().exiting.load(Ordering::Acquire) {
                break;
            }
            let next = {
                let state = app.state::<AppState>();
                let Ok(mut guard) = state.runtime.lock() else {
                    continue;
                };
                guard
                    .as_mut()
                    .and_then(|runtime| runtime.status_has_changed().then(|| runtime.status()))
            };
            if let Some(status) = next {
                let _ = app.emit(STATUS_EVENT, status);
            }
        }
    });
}

fn stop_for_exit(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    state.exiting.store(true, Ordering::Release);
    if let Ok(mut guard) = state.runtime.lock() {
        if let Some(mut runtime) = guard.take() {
            runtime.stop();
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let path = state_path(&app.handle()).map_err(|error| std::io::Error::other(error))?;
            app.manage(AppState {
                runtime: Mutex::new(None),
                state_path: path,
                exiting: AtomicBool::new(false),
            });
            let open = MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?;
            let start = MenuItem::with_id(app, "start", "启动运行时", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "停止运行时", true, None::<&str>)?;
            let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &start, &stop, &exit])?;
            let handle = app.handle().clone();
            TrayIconBuilder::with_id("main")
                .tooltip("远程环境采集器")
                .menu(&menu)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "start" => {
                        let state = app.state::<AppState>();
                        let _ = start_runtime(app.clone(), state);
                    }
                    "stop" => {
                        let state = app.state::<AppState>();
                        let _ = stop_runtime(app.clone(), state);
                    }
                    "exit" => {
                        stop_for_exit(app);
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;
            spawn_status_bridge(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_runtime_status,
            get_desktop_config,
            save_server_profile,
            delete_server_profile,
            set_runtime_options,
            test_server_profile,
            start_runtime,
            stop_runtime
        ])
        .build(tauri::generate_context!())
        .expect("桌面应用初始化失败")
        .run(|app, event| match event {
            tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::CloseRequested { api, .. },
                label,
                ..
            } if label == "main" => {
                api.prevent_close();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            tauri::RunEvent::ExitRequested { .. } => stop_for_exit(app),
            _ => {}
        });
}
