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
pub struct ServerProfile {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub device_id: String,
    pub token: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ServerMode {
    #[default]
    Single,
    Multi,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClientConfig {
    pub server_url: String,
    pub token: String,
    #[serde(default)]
    pub server_profiles: Vec<ServerProfile>,
    #[serde(default)]
    pub server_mode: ServerMode,
    #[serde(default)]
    pub active_server_id: Option<String>,
    pub identity: DeviceIdentity,
    pub wifi_enabled: bool,
    #[serde(default)]
    pub bluetooth_enabled: bool,
    pub scan_interval_seconds: u64,
    #[serde(default = "default_upload_interval_seconds")]
    pub upload_interval_seconds: u64,
    pub heartbeat_interval_seconds: u64,
    pub max_uploads_per_minute: u64,
    pub max_queue_size: u64,
    pub log_level: LoggingLevel,
}

fn default_upload_interval_seconds() -> u64 {
    30
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_url: "ws://127.0.0.1:8000/ws".into(),
            token: String::new(),
            server_profiles: Vec::new(),
            server_mode: ServerMode::Single,
            active_server_id: None,
            identity: DeviceIdentity {
                device_id: String::new(),
                device_name: "RemoteEnvCollector".into(),
                platform: std::env::consts::OS.into(),
                platform_version: String::new(),
                client_version: env!("CARGO_PKG_VERSION").into(),
                hardware: None,
            },
            wifi_enabled: false,
            bluetooth_enabled: false,
            scan_interval_seconds: 30,
            upload_interval_seconds: 30,
            heartbeat_interval_seconds: 5,
            max_uploads_per_minute: 60,
            max_queue_size: 1000,
            log_level: LoggingLevel::Info,
        }
    }
}

impl ClientConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.server_url.trim().is_empty() && self.server_profiles.is_empty() {
            return Err("server_url is required".into());
        }
        if self.token.trim().is_empty() && self.server_profiles.is_empty() {
            return Err("token is required".into());
        }
        if self.identity.device_id.trim().is_empty() {
            return Err("device_id is required".into());
        }
        if self.scan_interval_seconds == 0
            || self.upload_interval_seconds == 0
            || self.heartbeat_interval_seconds == 0
        {
            return Err("intervals must be positive".into());
        }
        if self.max_queue_size == 0
            || self.max_uploads_per_minute == 0
            || self.max_uploads_per_minute > 60
        {
            return Err("max_queue_size and max_uploads_per_minute are invalid".into());
        }
        if self.server_mode == ServerMode::Single
            && !self.server_profiles.is_empty()
            && self.active_server_id.is_none()
        {
            return Err("active_server_id is required in Single server mode".into());
        }
        let selected = self.selected_servers();
        if selected
            .iter()
            .any(|profile| profile.device_id.trim().is_empty())
        {
            return Err("selected server device_id is required".into());
        }
        if let Some(active_id) = self.active_server_id.as_deref()
            && !self
                .server_profiles
                .iter()
                .any(|profile| profile.id == active_id)
        {
            return Err("active_server_id must reference an existing server profile".into());
        }
        for profile in &self.server_profiles {
            if profile.id.trim().is_empty() || profile.name.trim().is_empty() {
                return Err("server profile id and name are required".into());
            }
            if profile.url.trim().is_empty()
                || profile.device_id.trim().is_empty()
                || profile.token.trim().is_empty()
            {
                return Err("server profile url, device_id and token are required".into());
            }
        }
        Ok(())
    }

    pub fn selected_servers(&self) -> Vec<&ServerProfile> {
        match self.server_mode {
            ServerMode::Single => self
                .active_server_id
                .as_deref()
                .and_then(|id| self.server_profiles.iter().find(|p| p.id == id))
                .into_iter()
                .collect(),
            ServerMode::Multi => self.server_profiles.iter().filter(|p| p.enabled).collect(),
        }
    }
}
