use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EnvironmentEnvelope {
    pub r#type: String,
    pub version: u8,
    pub device_id: String,
    pub data_type: String,
    pub timestamp: i64,
    pub sequence: u64,
    pub data: Value,
}

impl EnvironmentEnvelope {
    pub fn new(
        device_id: impl Into<String>,
        data_type: impl Into<String>,
        sequence: u64,
        data: Value,
    ) -> Self {
        Self {
            r#type: "environment_data".into(),
            version: 1,
            device_id: device_id.into(),
            data_type: data_type.into(),
            timestamp: now_ms(),
            sequence,
            data,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ack {
    pub device_id: String,
    pub data_type: String,
    pub sequence: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ErrorFrame {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRegistration {
    pub name: String,
    pub device_type: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthFrame {
    pub r#type: String,
    pub role: String,
    pub token: String,
    pub device_id: String,
    pub device: DeviceRegistration,
}

impl AuthFrame {
    pub fn collector(
        token: impl Into<String>,
        identity: &crate::config::DeviceIdentity,
        capabilities: Vec<String>,
    ) -> Self {
        Self {
            r#type: "auth".into(),
            role: "collector".into(),
            token: token.into(),
            device_id: identity.device_id.clone(),
            device: DeviceRegistration {
                name: identity.device_name.clone(),
                device_type: "generic".into(),
                capabilities,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeartbeatFrame {
    pub r#type: String,
    pub timestamp: i64,
}

impl ErrorFrame {
    pub fn requires_sequence_recovery(&self) -> bool {
        self.code == "sequence_rejected"
    }
}

pub fn matches_ack(ack: &Ack, envelope: &EnvironmentEnvelope) -> bool {
    ack.device_id == envelope.device_id
        && ack.data_type == envelope.data_type
        && ack.sequence == envelope.sequence
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
