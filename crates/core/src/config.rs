use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum LoggingLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceIdentity {
    pub device_id: String,
    pub device_name: String,
    pub platform: String,
    pub platform_version: String,
    pub client_version: String,
    pub hardware: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientConfig {
    pub server_url: String,
    pub token: String,
    pub identity: DeviceIdentity,
    pub wifi_enabled: bool,
    pub ble_enabled: bool,
    pub classic_bluetooth_enabled: bool,
    pub scan_interval_seconds: u64,
    pub heartbeat_interval_seconds: u64,
    pub max_uploads_per_minute: u64,
    pub max_queue_size: u64,
    pub log_level: LoggingLevel,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://127.0.0.1:8000/ws".into(),
            token: String::new(),
            identity: DeviceIdentity {
                device_id: String::new(),
                device_name: "RemoteEnvCollector".into(),
                platform: std::env::consts::OS.into(),
                platform_version: String::new(),
                client_version: env!("CARGO_PKG_VERSION").into(),
                hardware: None,
            },
            wifi_enabled: false,
            ble_enabled: false,
            classic_bluetooth_enabled: false,
            scan_interval_seconds: 30,
            heartbeat_interval_seconds: 15,
            max_uploads_per_minute: 60,
            max_queue_size: 1000,
            log_level: LoggingLevel::Info,
        }
    }
}

impl ClientConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.server_url.trim().is_empty() {
            return Err("server_url is required".into());
        }
        if self.token.trim().is_empty() {
            return Err("token is required".into());
        }
        if self.identity.device_id.trim().is_empty() {
            return Err("device_id is required".into());
        }
        if self.scan_interval_seconds == 0 || self.heartbeat_interval_seconds == 0 {
            return Err("intervals must be positive".into());
        }
        if self.max_queue_size == 0
            || self.max_uploads_per_minute == 0
            || self.max_uploads_per_minute > 60
        {
            return Err("max_queue_size and max_uploads_per_minute are invalid".into());
        }
        Ok(())
    }
}
