use futures_util::{SinkExt, StreamExt};
use remote_env_core::collector::CollectorEvent;
use remote_env_core::config::{ClientConfig, ServerMode, ServerProfile};
use remote_env_core::protocol::AuthFrame;
#[cfg(windows)]
use remote_env_core::runtime::CollectorScan;
use remote_env_core::runtime::{RuntimeStatus, RuntimeSupervisor};
use remote_env_core::state::StateStore;
#[cfg(windows)]
use remote_env_platform_windows::bluetooth::{
    BluetoothCollector, NativeBleScanner, NativeClassicBluetoothScanner,
};
#[cfg(windows)]
use remote_env_platform_windows::wifi::{NativeWlanProvider, WlanProvider};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager, State};
#[cfg(desktop)]
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};
#[cfg(not(target_os = "android"))]
use tokio_tungstenite::connect_async;
#[cfg(target_os = "android")]
use tokio_tungstenite::connect_async_tls_with_config;
use tokio_tungstenite::tungstenite::Message;

const STATUS_EVENT: &str = "runtime_status_changed";
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(windows)]
fn make_bluetooth_scan() -> CollectorScan {
    let collector = std::sync::Arc::new(BluetoothCollector::new(
        NativeBleScanner::new(),
        NativeClassicBluetoothScanner::new(),
    ));
    std::sync::Arc::new(move || collector.scan_once().map_err(|error| error.to_string()))
}

fn make_runtime(
    config: ClientConfig,
    store: StateStore,
    app: &tauri::AppHandle,
) -> Result<RuntimeSupervisor, String> {
    #[cfg(windows)]
    {
        let _ = app;
        return RuntimeSupervisor::start_with_collectors(
            config,
            store,
            Some(std::sync::Arc::new(|| {
                NativeWlanProvider::new()
                    .scan()
                    .and_then(|snapshot| {
                        Ok(CollectorEvent {
                            data_type: "wifi".into(),
                            timestamp_ms: now_ms(),
                            data: serde_json::to_value(snapshot).map_err(|error| {
                                remote_env_platform_windows::wifi::WiFiError::InvalidData(
                                    error.to_string(),
                                )
                            })?,
                        })
                    })
                    .map_err(|error| error.to_string())
            })),
            Some(make_bluetooth_scan()),
        )
        .map_err(user_error);
    }
    #[cfg(target_os = "android")]
    {
        let collector = app
            .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
            .inner()
            .clone();
        let batch = std::sync::Arc::new(move || collector.collect_all());
        return RuntimeSupervisor::start_with_collectors_and_batch(
            config,
            store,
            None,
            None,
            Some(batch),
        )
        .map_err(user_error);
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = app;
        RuntimeSupervisor::start(config, store).map_err(user_error)
    }
}

pub struct AppState {
    runtime: Mutex<Option<RuntimeSupervisor>>,
    state_path: PathBuf,
    state_cache_path: PathBuf,
    log_path: PathBuf,
    exiting: AtomicBool,
    _instance_guard: SingleInstanceGuard,
}

#[cfg(windows)]
struct SingleInstanceGuard(isize);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(
                self.0 as windows_sys::Win32::Foundation::HANDLE,
            );
        }
    }
}

#[cfg(not(windows))]
struct SingleInstanceGuard;

fn acquire_instance_guard() -> Result<SingleInstanceGuard, String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::{
            Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
            System::Threading::CreateMutexW,
        };
        let name: Vec<u16> = std::ffi::OsStr::new("Global\\RemoteEnvCollector.SingleInstance")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err("无法创建应用单实例锁。".into());
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            return Err("RemoteEnvCollector 已在运行，请使用现有窗口或托盘实例。".into());
        }
        return Ok(SingleInstanceGuard(handle as isize));
    }
    #[cfg(not(windows))]
    {
        Ok(SingleInstanceGuard)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerProfileView {
    id: String,
    name: String,
    url: String,
    device_id: String,
    enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct DesktopConfigView {
    device_id: String,
    server_mode: ServerMode,
    active_server_id: Option<String>,
    server_profiles: Vec<ServerProfileView>,
    scan_interval_seconds: u64,
    upload_interval_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ServerProfileInput {
    id: Option<String>,
    name: String,
    url: String,
    device_id: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct ConnectionTestResult {
    success: bool,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct PersistenceSettingsView {
    is_android: bool,
    foreground_enabled: bool,
    auto_start_enabled: bool,
    hide_from_recents: bool,
    accessibility_enabled: bool,
    battery_optimization_ignored: bool,
    root_enabled: bool,
    root_available: bool,
    device_admin_active: bool,
    device_owner_active: bool,
    profile_owner_active: bool,
    dhizuku_compat_enabled: bool,
    dhizuku_supported: bool,
    android_api_level: u32,
    background_location_granted: bool,
    location_enabled: bool,
    device_owner_command: String,
}

#[tauri::command]
fn get_persistence_settings(app: tauri::AppHandle) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    {
        let settings = app
            .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
            .persistence_settings()?;
        return Ok(PersistenceSettingsView {
            is_android: true,
            foreground_enabled: settings.foreground_enabled,
            auto_start_enabled: settings.auto_start_enabled,
            hide_from_recents: settings.hide_from_recents,
            accessibility_enabled: settings.accessibility_enabled,
            battery_optimization_ignored: settings.battery_optimization_ignored,
            root_enabled: settings.root_enabled,
            root_available: settings.root_available,
            device_admin_active: settings.device_admin_active,
            device_owner_active: settings.device_owner_active,
            profile_owner_active: settings.profile_owner_active,
            dhizuku_compat_enabled: settings.dhizuku_compat_enabled,
            dhizuku_supported: settings.dhizuku_supported,
            android_api_level: settings.android_api_level,
            background_location_granted: settings.background_location_granted,
            location_enabled: settings.location_enabled,
            device_owner_command: settings.device_owner_command,
        });
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(PersistenceSettingsView {
            is_android: false,
            foreground_enabled: false,
            auto_start_enabled: false,
            hide_from_recents: false,
            accessibility_enabled: false,
            battery_optimization_ignored: false,
            root_enabled: false,
            root_available: false,
            device_admin_active: false,
            device_owner_active: false,
            profile_owner_active: false,
            dhizuku_compat_enabled: false,
            dhizuku_supported: false,
            android_api_level: 0,
            background_location_granted: false,
            location_enabled: false,
            device_owner_command: String::new(),
        })
    }
}

#[tauri::command]
fn set_foreground_service_enabled(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_foreground_enabled(enabled)?;
    #[cfg(not(target_os = "android"))]
    let _ = enabled;
    get_persistence_settings(app)
}

#[tauri::command]
fn set_auto_start_enabled(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_auto_start_enabled(enabled)?;
    #[cfg(not(target_os = "android"))]
    let _ = enabled;
    get_persistence_settings(app)
}

#[tauri::command]
fn request_auto_start_permission(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_auto_start_permission();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn set_hide_from_recents(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_hide_from_recents(enabled)?;
    #[cfg(not(target_os = "android"))]
    let _ = enabled;
    get_persistence_settings(app)
}

#[tauri::command]
fn request_accessibility_permission(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_accessibility_permission();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn request_home_settings(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_home_settings();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn request_battery_optimization_exemption(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_battery_optimization_exemption();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn set_root_support_enabled(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_root_enabled(enabled)?;
    #[cfg(not(target_os = "android"))]
    let _ = enabled;
    get_persistence_settings(app)
}

#[tauri::command]
fn request_device_admin(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_device_admin();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn request_background_location(app: tauri::AppHandle) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .request_background_location();
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(())
    }
}

#[tauri::command]
fn set_dhizuku_compat_enabled(
    enabled: bool,
    app: tauri::AppHandle,
) -> Result<PersistenceSettingsView, String> {
    #[cfg(target_os = "android")]
    app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_dhizuku_compat_enabled(enabled)?;
    #[cfg(not(target_os = "android"))]
    let _ = enabled;
    get_persistence_settings(app)
}

#[tauri::command]
fn list_dhizuku_apps(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    #[cfg(target_os = "android")]
    return serde_json::to_value(
        app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
            .list_dhizuku_apps()?,
    )
    .map_err(|error| error.to_string());
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(serde_json::json!([]))
    }
}

#[tauri::command]
fn set_dhizuku_app_authorization(
    package_name: String,
    allowed: bool,
    app: tauri::AppHandle,
) -> Result<(), String> {
    #[cfg(target_os = "android")]
    return app
        .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
        .set_dhizuku_app_authorization(&package_name, allowed);
    #[cfg(not(target_os = "android"))]
    {
        let _ = (package_name, allowed, app);
        Err("仅 Android 支持此功能。".into())
    }
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
    fs::create_dir_all(&dir).map_err(user_error)?;
    Ok(dir.join("state.sqlite3"))
}

fn state_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(user_error)?;
    fs::create_dir_all(&dir).map_err(user_error)?;
    Ok(dir.join("state_cache.sqlite3"))
}

fn log_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(user_error)?.join("logs");
    fs::create_dir_all(&dir).map_err(user_error)?;
    Ok(dir.join("collector.log"))
}

fn init_logging(path: PathBuf) {
    let _ = LOG_PATH.set(path);
}

fn write_log(level: &str, message: &str) {
    let Some(path) = LOG_PATH.get() else {
        return;
    };
    let safe = message
        .replace("Authorization", "[REDACTED]")
        .replace("Cookie", "[REDACTED]");
    if let Ok(metadata) = fs::metadata(path) {
        if metadata.len() > 5 * 1024 * 1024 {
            let rotated = path.with_extension("log.1");
            let _ = fs::rename(path, rotated);
        }
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "{} [{}] {}", now, level, safe);
    }
}

fn info(message: impl AsRef<str>) {
    write_log("INFO", message.as_ref());
}
fn warn(message: impl AsRef<str>) {
    write_log("WARN", message.as_ref());
}
fn error(message: impl AsRef<str>) {
    write_log("ERROR", message.as_ref());
}

#[cfg(target_os = "android")]
mod android_headless {
    use super::*;
    use jni::{
        JNIEnv,
        objects::{JClass, JString},
        sys::{jlong, jstring},
    };
    use std::{path::Path, ptr, sync::OnceLock};

    #[derive(Default)]
    struct HeadlessState {
        runtime: Option<RuntimeSupervisor>,
        data_dir: Option<PathBuf>,
        scan_interval_seconds: u64,
        wifi_runtime: remote_env_core::runtime::WiFiRuntimeStatus,
        bluetooth_runtime: remote_env_core::runtime::BluetoothRuntimeStatus,
        wifi_snapshot: Option<serde_json::Value>,
        bluetooth_snapshot: Option<serde_json::Value>,
        cell_snapshot: Option<serde_json::Value>,
        gps_snapshot: Option<serde_json::Value>,
        gnss_snapshot: Option<serde_json::Value>,
        status_dirty: bool,
    }

    static STATE: OnceLock<Mutex<HeadlessState>> = OnceLock::new();

    fn state() -> &'static Mutex<HeadlessState> {
        STATE.get_or_init(|| Mutex::new(HeadlessState::default()))
    }

    fn paths(data_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
        (
            data_dir.join("state.sqlite3"),
            data_dir.join("state_cache.sqlite3"),
            data_dir.join("logs").join("collector.log"),
        )
    }

    pub(super) fn start(data_dir: PathBuf) -> Result<RuntimeStatus, String> {
        fs::create_dir_all(&data_dir).map_err(user_error)?;
        let (config_path, cache_path, log_path) = paths(&data_dir);
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent).map_err(user_error)?;
        }
        init_logging(log_path);
        let (store, config) = load_config(&config_path, &cache_path)?;
        if config.selected_servers().is_empty() {
            return Err("请先新增并启用至少一个服务器配置。".into());
        }
        let mut headless = state()
            .lock()
            .map_err(|_| "后台服务状态不可用。".to_string())?;
        headless.scan_interval_seconds = config.scan_interval_seconds.max(1);
        headless.data_dir = Some(data_dir);
        if let Some(runtime) = headless.runtime.as_ref() {
            runtime
                .set_collection_running(config, true)
                .map_err(user_error)?;
            return Ok(runtime.status());
        }
        let runtime = RuntimeSupervisor::start(config.clone(), store).map_err(user_error)?;
        runtime
            .set_collection_running(config, true)
            .map_err(user_error)?;
        let status = runtime.status();
        headless.runtime = Some(runtime);
        info("Android headless 服务运行时已启动");
        Ok(status)
    }

    pub(super) fn stop() -> Result<(), String> {
        let mut headless = state()
            .lock()
            .map_err(|_| "后台服务状态不可用。".to_string())?;
        if let Some(mut runtime) = headless.runtime.take() {
            runtime.stop();
        }
        info("Android headless 服务运行时已停止");
        Ok(())
    }

    pub(super) fn submit(events_json: &str) -> Result<(), String> {
        let events: Vec<CollectorEvent> = serde_json::from_str(events_json).map_err(user_error)?;
        let mut headless = state()
            .lock()
            .map_err(|_| "后台服务状态不可用。".to_string())?;
        for event in events {
            let data_type = event.data_type.clone();
            let timestamp_ms = event.timestamp_ms;
            let data = event.data.clone();
            headless
                .runtime
                .as_ref()
                .ok_or_else(|| "后台服务运行时尚未启动。".to_string())?
                .submit(event)
                .map_err(user_error)?;
            match data_type.as_str() {
                "wifi" => {
                    headless.wifi_runtime.enabled = true;
                    headless.wifi_runtime.state = remote_env_core::runtime::WiFiRuntimeState::Ready;
                    headless.wifi_runtime.last_scan_ms = Some(timestamp_ms);
                    headless.wifi_runtime.last_successful_scan_ms = Some(timestamp_ms);
                    headless.wifi_runtime.network_count = data["networks"].as_array().map(Vec::len);
                    headless.wifi_runtime.last_error = None;
                    headless.wifi_runtime.total_scans += 1;
                    headless.wifi_runtime.successful_scans += 1;
                    headless.wifi_snapshot = Some(data);
                }
                "bluetooth" => {
                    let devices = data["devices"].as_array();
                    headless.bluetooth_runtime.enabled = true;
                    headless.bluetooth_runtime.state =
                        remote_env_core::runtime::WiFiRuntimeState::Ready;
                    headless.bluetooth_runtime.device_count = devices.map(Vec::len);
                    headless.bluetooth_runtime.ble_device_count = devices.map_or(0, |items| {
                        items
                            .iter()
                            .filter(|item| {
                                matches!(item["mode"].as_str(), Some("ble") | Some("dual"))
                            })
                            .count()
                    });
                    headless.bluetooth_runtime.classic_device_count = devices.map_or(0, |items| {
                        items
                            .iter()
                            .filter(|item| {
                                matches!(item["mode"].as_str(), Some("classic") | Some("dual"))
                            })
                            .count()
                    });
                    headless.bluetooth_runtime.last_scan_ms = Some(timestamp_ms);
                    headless.bluetooth_runtime.last_successful_scan_ms = Some(timestamp_ms);
                    headless.bluetooth_runtime.last_error = None;
                    headless.bluetooth_runtime.total_scans += 1;
                    headless.bluetooth_runtime.successful_scans += 1;
                    headless.bluetooth_snapshot = Some(data);
                }
                "cell" => headless.cell_snapshot = Some(data),
                "gps" => headless.gps_snapshot = Some(data),
                "gnss" => headless.gnss_snapshot = Some(data),
                _ => {}
            }
        }
        headless.status_dirty = true;
        Ok(())
    }

    pub(super) fn status() -> RuntimeStatus {
        state().lock().map_or_else(
            |_| RuntimeStatus::default(),
            |headless| decorated_status(&headless),
        )
    }

    pub(super) fn status_if_changed() -> Option<RuntimeStatus> {
        state().lock().ok().and_then(|mut headless| {
            let runtime_changed = headless
                .runtime
                .as_mut()
                .is_some_and(RuntimeSupervisor::status_has_changed);
            if runtime_changed || headless.status_dirty {
                headless.status_dirty = false;
                Some(decorated_status(&headless))
            } else {
                None
            }
        })
    }

    fn decorated_status(headless: &HeadlessState) -> RuntimeStatus {
        let mut status = headless
            .runtime
            .as_ref()
            .map(RuntimeSupervisor::status)
            .unwrap_or_default();
        if headless.wifi_runtime.enabled {
            status.wifi = remote_env_core::runtime::CollectorStatus::Ready;
            status.wifi_runtime = headless.wifi_runtime.clone();
        }
        if headless.bluetooth_runtime.enabled {
            status.bluetooth = remote_env_core::runtime::CollectorStatus::Ready;
            status.bluetooth_runtime = headless.bluetooth_runtime.clone();
        }
        status.wifi_snapshot = headless.wifi_snapshot.clone();
        status.bluetooth_snapshot = headless.bluetooth_snapshot.clone();
        status.cell_snapshot = headless.cell_snapshot.clone();
        status.gps_snapshot = headless.gps_snapshot.clone();
        status.gnss_snapshot = headless.gnss_snapshot.clone();
        status
    }

    pub(super) fn reconfigure(config: &ClientConfig) -> Result<(), String> {
        let mut headless = state()
            .lock()
            .map_err(|_| "后台服务状态不可用。".to_string())?;
        headless.scan_interval_seconds = config.scan_interval_seconds.max(1);
        if let Some(runtime) = headless.runtime.as_ref() {
            runtime.update_config(config.clone()).map_err(user_error)?;
        }
        Ok(())
    }

    fn interval_millis() -> i64 {
        state()
            .lock()
            .map(|headless| headless.scan_interval_seconds.max(1) as i64 * 1_000)
            .unwrap_or(30_000)
    }

    fn java_string(env: &mut JNIEnv<'_>, value: impl AsRef<str>) -> jstring {
        env.new_string(value.as_ref())
            .map(|value| value.into_raw())
            .unwrap_or(ptr::null_mut())
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_remoteenv_collector_nativecollector_HeadlessRuntime_nativeStart(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        data_dir: JString<'_>,
    ) -> jstring {
        let result = env
            .get_string(&data_dir)
            .map(|value| PathBuf::from(value.to_string_lossy().into_owned()))
            .map_err(|error| error.to_string())
            .and_then(start)
            .and_then(|status| serde_json::to_string(&status).map_err(user_error));
        java_string(
            &mut env,
            result.unwrap_or_else(|error| {
                format!(
                    r#"{{"error":{}}}"#,
                    serde_json::to_string(&error).unwrap_or_else(|_| "\"unknown\"".into())
                )
            }),
        )
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_remoteenv_collector_nativecollector_HeadlessRuntime_nativeStop(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) -> jstring {
        let result = stop()
            .map(|_| "ok".to_string())
            .unwrap_or_else(|error| error);
        java_string(&mut env, result)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_remoteenv_collector_nativecollector_HeadlessRuntime_nativeSubmitEvents(
        mut env: JNIEnv<'_>,
        _class: JClass<'_>,
        events: JString<'_>,
    ) -> jstring {
        let result = env
            .get_string(&events)
            .map(|value| value.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())
            .and_then(|events| submit(&events))
            .map(|_| "ok".to_string())
            .unwrap_or_else(|error| error);
        java_string(&mut env, result)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_remoteenv_collector_nativecollector_HeadlessRuntime_nativeIntervalMillis(
        _env: JNIEnv<'_>,
        _class: JClass<'_>,
    ) -> jlong {
        interval_millis() as jlong
    }
}

#[tauri::command]
fn get_log_tail(state: State<'_, AppState>) -> Result<String, String> {
    let content = fs::read_to_string(&state.log_path).map_err(user_error)?;
    Ok(content
        .lines()
        .rev()
        .take(200)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

#[tauri::command]
fn get_log_path(state: State<'_, AppState>) -> String {
    state.log_path.display().to_string()
}

#[derive(serde::Serialize)]
pub struct StateInfoView {
    pub config_db_path: String,
    pub config_db_bytes: u64,
    pub state_cache_db_path: String,
    pub state_cache_db_bytes: u64,
}

#[derive(serde::Serialize)]
pub struct CleanupStateCacheResult {
    pub removed_rows: u64,
    pub state_cache_db_bytes: u64,
}

fn file_size_bytes(path: &PathBuf) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Reports both database paths and sizes so the UI can show database health.
/// The user configuration database stays small; the state cache is rebuildable.
#[tauri::command]
fn get_state_info(state: State<'_, AppState>) -> StateInfoView {
    StateInfoView {
        config_db_path: state.state_path.display().to_string(),
        config_db_bytes: file_size_bytes(&state.state_path),
        state_cache_db_path: state.state_cache_path.display().to_string(),
        state_cache_db_bytes: file_size_bytes(&state.state_cache_path),
    }
}

/// Manually prunes acknowledged/cancelled upload cache rows older than 24 hours
/// and compacts the state cache database. User configuration is never touched.
/// If the state cache is unreadable it is deleted and rebuilt automatically.
#[tauri::command]
fn cleanup_state_cache(state: State<'_, AppState>) -> Result<CleanupStateCacheResult, String> {
    let store = open_store(&state.state_path, &state.state_cache_path)?;
    let removed = store
        .cleanup_state_cache(std::time::Duration::from_secs(24 * 3600))
        .map_err(user_error)?;
    Ok(CleanupStateCacheResult {
        removed_rows: removed,
        state_cache_db_bytes: file_size_bytes(&state.state_cache_path),
    })
}

fn open_store(config_path: &PathBuf, state_cache_path: &PathBuf) -> Result<StateStore, String> {
    StateStore::open(config_path, state_cache_path).map_err(user_error)
}

fn load_config(
    path: &PathBuf,
    state_cache_path: &PathBuf,
) -> Result<(StateStore, ClientConfig), String> {
    let store = open_store(path, state_cache_path)?;
    let mut config = store.load_config().map_err(user_error)?.unwrap_or_default();
    for profile in &mut config.server_profiles {
        profile.token = protect::decrypt(&profile.token).map_err(user_error)?;
    }
    let mut heartbeat_migrated = false;
    if config.heartbeat_interval_seconds != 5 {
        config.heartbeat_interval_seconds = 5;
        heartbeat_migrated = true;
    }
    let mut migrated = false;
    if !config.identity.device_id.is_empty() {
        for profile in &mut config.server_profiles {
            if profile.device_id.is_empty() {
                profile.device_id = config.identity.device_id.clone();
                migrated = true;
            }
        }
    }
    if config.identity.device_id.is_empty() {
        let platform = if cfg!(target_os = "android") {
            "android"
        } else if cfg!(windows) {
            "windows"
        } else {
            std::env::consts::OS
        };
        config.identity = store
            .load_or_create_identity("RemoteEnvCollector", platform, "")
            .map_err(user_error)?;
        save_config(&store, &config)?;
    } else if migrated || heartbeat_migrated {
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
                device_id: profile.device_id.clone(),
                enabled: profile.enabled,
            })
            .collect(),
        scan_interval_seconds: config.scan_interval_seconds,
        upload_interval_seconds: config.upload_interval_seconds,
    }
}

fn update_runtime(state: &AppState, config: &ClientConfig) -> Result<(), String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        return android_headless::reconfigure(config);
    }
    #[cfg(not(target_os = "android"))]
    {
        let guard = state
            .runtime
            .lock()
            .map_err(|_| "应用状态不可用。".to_string())?;
        if let Some(runtime) = guard.as_ref() {
            runtime.update_config(config.clone()).map_err(user_error)?;
        }
        Ok(())
    }
}

fn current_status(state: &AppState) -> Result<RuntimeStatus, String> {
    #[cfg(target_os = "android")]
    {
        let _ = state;
        return Ok(android_headless::status());
    }
    #[cfg(not(target_os = "android"))]
    {
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
}

#[tauri::command]
fn get_runtime_status(state: State<'_, AppState>) -> Result<RuntimeStatus, String> {
    current_status(&state).map_err(|message| {
        error(format!("get_runtime_status 失败: {message}"));
        message
    })
}

#[tauri::command]
fn get_desktop_config(state: State<'_, AppState>) -> Result<DesktopConfigView, String> {
    let (_, config) =
        load_config(&state.state_path, &state.state_cache_path).map_err(|message| {
            error(format!("get_desktop_config 失败: {message}"));
            message
        })?;
    Ok(config_view(&config))
}

#[tauri::command]
fn save_server_profile(
    input: ServerProfileInput,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    let name = input.name.trim();
    let url = input.url.trim();
    let device_id = input.device_id.as_deref().unwrap_or("").trim();
    if name.is_empty() || url.is_empty() || device_id.is_empty() {
        return Err("服务器名称、WebSocket 地址和设备 ID 不能为空。".into());
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
        profile.device_id = device_id.into();
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
            device_id: device_id.into(),
            token,
            enabled: true,
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

#[tauri::command(async)]
fn disconnect_server_profile(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    let Some(profile) = config
        .server_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    else {
        return Err("未找到服务器配置。".into());
    };
    profile.enabled = false;
    if config.active_server_id.as_deref() == Some(id.as_str()) {
        config.active_server_id = config
            .server_profiles
            .iter()
            .find(|item| item.id != id && item.enabled)
            .map(|item| item.id.clone());
    }
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    let status = current_status(&state)?;
    let _ = app.emit(STATUS_EVENT, &status);
    Ok(status)
}

#[tauri::command(async)]
fn set_server_enabled(
    id: String,
    enabled: bool,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    if let Some(profile) = config
        .server_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    {
        profile.enabled = enabled;
    } else {
        return Err("未找到服务器配置。".into());
    }
    if enabled && config.server_mode == ServerMode::Single {
        config.active_server_id = Some(id.clone());
    }
    if !enabled && config.active_server_id.as_deref() == Some(id.as_str()) {
        config.active_server_id = None;
    }
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    let status = current_status(&state)?;
    let _ = app.emit(STATUS_EVENT, &status);
    Ok(status)
}

#[tauri::command]
fn delete_server_profile(
    id: String,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    config.server_profiles.retain(|profile| profile.id != id);
    if config.active_server_id.as_deref() == Some(&id) {
        config.active_server_id = config
            .server_profiles
            .iter()
            .find(|profile| profile.enabled)
            .map(|profile| profile.id.clone());
    }
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    Ok(config_view(&config))
}

#[tauri::command(async)]
fn set_runtime_options(
    server_mode: ServerMode,
    active_server_id: Option<String>,
    scan_interval_seconds: u64,
    upload_interval_seconds: u64,
    state: State<'_, AppState>,
) -> Result<DesktopConfigView, String> {
    if scan_interval_seconds == 0
        || scan_interval_seconds > 3600
        || upload_interval_seconds == 0
        || upload_interval_seconds > 3600
    {
        return Err("扫描间隔和上传间隔必须在 1 到 3600 秒之间。".into());
    }
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    config.server_mode = server_mode;
    config.active_server_id = active_server_id;
    config.scan_interval_seconds = scan_interval_seconds;
    config.upload_interval_seconds = upload_interval_seconds;
    if config.server_mode == ServerMode::Single && !config.server_profiles.is_empty() {
        let active_is_valid = config.active_server_id.as_deref().is_some_and(|active| {
            config
                .server_profiles
                .iter()
                .any(|profile| profile.enabled && profile.id == active)
        });
        if !active_is_valid {
            config.active_server_id = config
                .server_profiles
                .iter()
                .find(|profile| profile.enabled)
                .map(|profile| profile.id.clone());
        }
        if config.active_server_id.is_none() {
            if let Some(profile) = config.server_profiles.first_mut() {
                profile.enabled = true;
                config.active_server_id = Some(profile.id.clone());
            }
        }
        if config.active_server_id.is_none() {
            return Err("单服务器模式必须选择有效的活动服务器。".into());
        }
    }
    config.validate().map_err(user_error)?;
    update_runtime(&state, &config)?;
    save_config(&store, &config)?;
    Ok(config_view(&config))
}

#[tauri::command(async)]
fn connect_server_profile(
    id: String,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    info(format!("收到服务器连接请求 profile_id={id}"));
    let (store, mut config) = load_config(&state.state_path, &state.state_cache_path)?;
    let profile = config
        .server_profiles
        .iter()
        .find(|profile| profile.id == id)
        .ok_or_else(|| "未找到服务器配置。".to_string())?;
    if profile.device_id.trim().is_empty() || profile.token.trim().is_empty() {
        warn(format!("服务器配置不完整 profile_id={id}"));
        return Err("请先完善服务器的设备 ID 和令牌。".into());
    }
    if config.server_mode == ServerMode::Single {
        config.active_server_id = Some(id.clone());
    }
    if let Some(profile) = config
        .server_profiles
        .iter_mut()
        .find(|profile| profile.id == id)
    {
        profile.enabled = true;
    }
    config.validate().map_err(user_error)?;
    save_config(&store, &config)?;
    let mut guard = state
        .runtime
        .lock()
        .map_err(|_| "应用状态不可用。".to_string())?;
    if let Some(runtime) = guard.as_ref() {
        runtime.update_config(config.clone()).map_err(user_error)?;
    } else {
        *guard = Some(make_runtime(config.clone(), store, &app)?);
    }
    let initial_status = guard
        .as_ref()
        .map(RuntimeSupervisor::status)
        .unwrap_or_default();
    let mut status = initial_status;
    if let Some(server) = status
        .servers
        .iter_mut()
        .find(|server| server.profile_id == id)
    {
        server.connection = remote_env_core::transport::ConnectionState::Connecting;
        server.heartbeat_alive = false;
        server.last_heartbeat_ms = None;
    } else {
        status
            .servers
            .push(remote_env_core::worker::ServerWorkerStatus {
                next_retry_at_ms: None,
                profile_id: id.clone(),
                connection: remote_env_core::transport::ConnectionState::Connecting,
                heartbeat_alive: false,
                last_heartbeat_ms: None,
                last_error: None,
                pending: 0,
                in_flight: 0,
                blocked: 0,
                uploaded: 0,
                failed: 0,
            });
    }
    status.connection = remote_env_core::transport::ConnectionState::Connecting;
    let _ = app.emit(STATUS_EVENT, &status);
    info("服务器连接命令已提交");
    Ok(status)
}

#[tauri::command]
#[cfg(windows)]
async fn scan_wifi_now() -> Result<CollectorEvent, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let result = NativeWlanProvider::new().scan();
        match result {
            Ok(snapshot) => {
                info(format!(
                    "Wi-Fi 单次扫描完成 networks={}",
                    snapshot.networks.len()
                ));
                Ok(CollectorEvent {
                    data_type: "wifi".into(),
                    timestamp_ms: 0,
                    data: serde_json::to_value(snapshot).map_err(|error| error.to_string())?,
                })
            }
            Err(scan_error) => {
                error(format!("Wi-Fi 单次扫描失败: {scan_error}"));
                Err(scan_error.to_string())
            }
        }
    })
    .await
    .map_err(|_| "Wi-Fi 扫描任务异常终止。".to_string())?
}

#[tauri::command]
#[cfg(not(windows))]
async fn scan_wifi_now() -> Result<CollectorEvent, String> {
    Err("移动端由批量原生采集器持续采集，请启动采集服务查看结果。".into())
}

#[tauri::command]
#[cfg(windows)]
async fn scan_bluetooth_now() -> Result<CollectorEvent, String> {
    tauri::async_runtime::spawn_blocking(|| {
        BluetoothCollector::new(
            NativeBleScanner::new(),
            NativeClassicBluetoothScanner::new(),
        )
        .scan_once()
        .map_err(|scan_error| {
            error(format!("蓝牙单次扫描失败: {scan_error}"));
            scan_error.to_string()
        })
    })
    .await
    .map_err(|_| "蓝牙扫描任务异常终止。".to_string())?
}

#[tauri::command]
#[cfg(not(windows))]
async fn scan_bluetooth_now() -> Result<CollectorEvent, String> {
    Err("移动端由批量原生采集器持续采集，请启动采集服务查看结果。".into())
}

#[tauri::command]
async fn scan_android_environment_now(
    data_type: String,
    app: tauri::AppHandle,
) -> Result<CollectorEvent, String> {
    #[cfg(target_os = "android")]
    {
        if !matches!(
            data_type.as_str(),
            "wifi" | "bluetooth" | "cell" | "gps" | "gnss"
        ) {
            return Err("不支持的数据类型。".into());
        }
        let collector = app
            .state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
            .inner()
            .clone();
        return tauri::async_runtime::spawn_blocking(move || collector.collect_all())
            .await
            .map_err(|_| "原生采集任务异常终止。".to_string())??
            .into_iter()
            .find(|event| event.data_type == data_type)
            .ok_or_else(|| "原生采集器未返回该类型数据。".to_string());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (data_type, app);
        Err("该采集入口仅适用于 Android。".into())
    }
}

#[tauri::command]
async fn test_server_profile(
    id: String,
    state: State<'_, AppState>,
) -> Result<ConnectionTestResult, String> {
    info(format!("收到服务器测试请求 profile_id={id}"));
    let (_, config) = load_config(&state.state_path, &state.state_cache_path)?;
    let profile = config
        .server_profiles
        .iter()
        .find(|profile| profile.id == id)
        .cloned()
        .ok_or_else(|| "未找到服务器配置。".to_string())?;
    let identity = remote_env_core::config::DeviceIdentity {
        device_id: profile.device_id.clone(),
        ..config.identity
    };
    let test = async move {
        #[cfg(target_os = "android")]
        let connection = connect_async_tls_with_config(
            &profile.url,
            None,
            false,
            Some(remote_env_core::transport::insecure_tls_connector()),
        )
        .await;
        #[cfg(not(target_os = "android"))]
        let connection = connect_async(&profile.url).await;
        let (mut socket, _) =
            connection.map_err(|error| format!("无法建立 WebSocket 连接：{error}"))?;
        let auth = AuthFrame::collector(
            profile.token,
            &identity,
            AuthFrame::platform_capabilities(&identity),
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
        Ok(ConnectionTestResult {
            success: true,
            message: "连接成功，认证成功，服务器可用。".into(),
        })
    };
    tokio::time::timeout(Duration::from_secs(10), test)
        .await
        .map_err(|_| "连接测试超时，请检查网络和服务器地址。".to_string())?
}

#[tauri::command]
fn start_runtime(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    info("收到启动采集服务请求");
    let (store, config) = load_config(&state.state_path, &state.state_cache_path)?;
    if config.selected_servers().is_empty() {
        return Err("请先新增并启用至少一个服务器配置。".into());
    }
    #[cfg(target_os = "android")]
    {
        drop(store);
        app.state::<remote_env_platform_android::AndroidCollector<tauri::Wry>>()
            .set_foreground_enabled(true)?;
        let data_dir = state
            .state_path
            .parent()
            .ok_or_else(|| "Android 数据目录不可用。".to_string())?
            .to_path_buf();
        let status = android_headless::start(data_dir)?;
        let _ = app.emit(STATUS_EVENT, &status);
        return Ok(status);
    }
    #[cfg(not(target_os = "android"))]
    {
        let mut guard = state
            .runtime
            .lock()
            .map_err(|_| "应用状态不可用。".to_string())?;
        if guard.is_none() {
            *guard = Some(make_runtime(config.clone(), store, &app)?);
        }
        if let Some(runtime) = guard.as_ref() {
            let command_result = runtime
                .set_collection_running(config.clone(), true)
                .map_err(user_error);
            if command_result.is_ok() {
                info("采集服务运行命令已发送，等待 Runtime 应用并启动扫描器");
            }
            command_result?;
        }
        let status = guard
            .as_ref()
            .map(RuntimeSupervisor::status)
            .map(|mut status| {
                status.collection_running = true;
                status
            })
            .unwrap_or_default();
        let _ = app.emit(STATUS_EVENT, &status);
        info("采集服务启动命令已提交，扫描器已请求立即运行");
        Ok(status)
    }
}

#[tauri::command]
fn stop_runtime(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<RuntimeStatus, String> {
    info("收到停止采集服务请求");
    #[cfg(target_os = "android")]
    {
        android_headless::stop()?;
        let status = current_status(&state)?;
        let _ = app.emit(STATUS_EVENT, &status);
        return Ok(status);
    }
    #[cfg(not(target_os = "android"))]
    {
        let runtime = {
            let mut guard = state
                .runtime
                .lock()
                .map_err(|_| "应用状态不可用。".to_string())?;
            guard.take()
        };
        if let Some(mut runtime) = runtime {
            runtime.stop();
            info("采集服务及其服务器、采集器已停止");
        }
        let status = current_status(&state)?;
        let _ = app.emit(STATUS_EVENT, &status);
        Ok(status)
    }
}

fn spawn_status_bridge(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut last_connection: Option<String> = None;
        let mut last_scan_counts = (0_u64, 0_u64, 0_u64, 0_u64);
        let mut last_heartbeats: std::collections::HashMap<String, Option<i64>> =
            std::collections::HashMap::new();
        loop {
            std::thread::sleep(Duration::from_millis(200));
            if app.state::<AppState>().exiting.load(Ordering::Acquire) {
                break;
            }
            let next = {
                #[cfg(target_os = "android")]
                {
                    android_headless::status_if_changed()
                }
                #[cfg(not(target_os = "android"))]
                {
                    let state = app.state::<AppState>();
                    let Ok(mut guard) = state.runtime.lock() else {
                        continue;
                    };
                    guard
                        .as_mut()
                        .and_then(|runtime| runtime.status_has_changed().then(|| runtime.status()))
                }
            };
            if let Some(status) = next {
                let connection = status.connection.to_string();
                let counts = (
                    status.wifi_runtime.successful_scans,
                    status.wifi_runtime.failed_scans,
                    status.bluetooth_runtime.successful_scans,
                    status.bluetooth_runtime.failed_scans,
                );
                if last_connection.as_deref() != Some(connection.as_str())
                    || counts != last_scan_counts
                {
                    info(format!(
                        "状态变化 connection={connection} collection_running={} wifi={:?} bluetooth={:?} wifi_scans={:?} bluetooth_scans={:?}",
                        status.collection_running,
                        status.wifi,
                        status.bluetooth,
                        status.wifi_runtime,
                        status.bluetooth_runtime
                    ));
                    last_connection = Some(connection);
                    last_scan_counts = counts;
                }
                for server in &status.servers {
                    let previous =
                        last_heartbeats.insert(server.profile_id.clone(), server.last_heartbeat_ms);
                    if previous != Some(server.last_heartbeat_ms) {
                        info(format!(
                            "服务器心跳更新 profile_id={} alive={} last_heartbeat_ms={:?}",
                            server.profile_id, server.heartbeat_alive, server.last_heartbeat_ms
                        ));
                    }
                }
                let _ = app.emit(STATUS_EVENT, status);
            }
        }
    });
}

fn stop_for_exit(app: &tauri::AppHandle) {
    #[cfg(target_os = "android")]
    {
        let _ = app;
        return;
    }
    #[cfg(not(target_os = "android"))]
    {
        let state = app.state::<AppState>();
        state.exiting.store(true, Ordering::Release);
        if let Ok(mut guard) = state.runtime.lock() {
            if let Some(mut runtime) = guard.take() {
                runtime.stop();
            }
        }
    }
}

pub fn run_app(tray_start: bool) {
    #[cfg(target_os = "android")]
    let builder = tauri::Builder::default().plugin(remote_env_platform_android::init());
    #[cfg(not(target_os = "android"))]
    let builder = tauri::Builder::default();
    builder
        .setup(move |app| {
            let path = state_path(&app.handle()).map_err(|error| std::io::Error::other(error))?;
            let cache =
                state_cache_path(&app.handle()).map_err(|error| std::io::Error::other(error))?;
            let log = log_path(&app.handle()).map_err(|error| std::io::Error::other(error))?;
            let guard = acquire_instance_guard().map_err(std::io::Error::other)?;
            app.manage(AppState {
                runtime: Mutex::new(None),
                state_path: path,
                state_cache_path: cache,
                log_path: log.clone(),
                exiting: AtomicBool::new(false),
                _instance_guard: guard,
            });
            init_logging(log);
            info("应用启动");
            let handle = app.handle().clone();
            #[cfg(desktop)]
            {
                let open = MenuItem::with_id(app, "open", "打开主窗口", true, None::<&str>)?;
                let start = MenuItem::with_id(app, "start", "启动采集服务", true, None::<&str>)?;
                let stop = MenuItem::with_id(app, "stop", "停止采集服务", true, None::<&str>)?;
                let exit = MenuItem::with_id(app, "exit", "退出", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &start, &stop, &exit])?;
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
            }
            if tray_start {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
                info("收到 --tray 参数，隐藏主窗口");
            }
            // Windows and Android share the same unattended-start rule: once a
            // selected server exists, start collection immediately on launch.
            let startup_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(500));
                let state = startup_handle.state::<AppState>();
                let configured = load_config(&state.state_path, &state.state_cache_path)
                    .map(|(_, config)| !config.selected_servers().is_empty())
                    .unwrap_or(false);
                if configured {
                    match start_runtime(startup_handle.clone(), state) {
                        Ok(status) => info(format!(
                            "检测到已配置服务器，自动采集已启动 collection_running={}",
                            status.collection_running
                        )),
                        Err(message) => warn(format!("启动时自动采集失败: {message}")),
                    }
                } else {
                    info("启动时未检测到已选服务器，保持采集停止");
                }
            });
            spawn_status_bridge(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_log_tail,
            get_log_path,
            get_runtime_status,
            get_desktop_config,
            save_server_profile,
            delete_server_profile,
            set_runtime_options,
            connect_server_profile,
            disconnect_server_profile,
            set_server_enabled,
            scan_wifi_now,
            scan_bluetooth_now,
            scan_android_environment_now,
            test_server_profile,
            get_state_info,
            cleanup_state_cache,
            get_persistence_settings,
            set_foreground_service_enabled,
            set_auto_start_enabled,
            request_auto_start_permission,
            set_hide_from_recents,
            request_accessibility_permission,
            request_home_settings,
            request_battery_optimization_exemption,
            set_root_support_enabled,
            request_device_admin,
            request_background_location,
            set_dhizuku_compat_enabled,
            list_dhizuku_apps,
            set_dhizuku_app_authorization,
            start_runtime,
            stop_runtime
        ])
        .build(tauri::generate_context!())
        .expect("桌面应用初始化失败")
        .run(move |app, event| match event {
            tauri::RunEvent::Ready => {
                if let Some(window) = app.get_webview_window("main") {
                    if tray_start {
                        // Tray auto-start: keep the main window hidden.
                        let _ = window.hide();
                    } else {
                        // Normal start: the window is created hidden by config,
                        // so reveal it here after the UI is ready.
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
            }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_app(false);
}
