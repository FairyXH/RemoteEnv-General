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
        Self::with_timestamp(
            device_id,
            data_type,
            now_ms(),
            sequence.max(now_ms() as u64),
            data,
        )
    }

    pub fn with_timestamp(
        device_id: impl Into<String>,
        data_type: impl Into<String>,
        timestamp: i64,
        mut sequence: u64,
        mut data: Value,
    ) -> Self {
        if sequence < now_ms() as u64 {
            sequence = now_ms() as u64;
        }
        let data_type = data_type.into();
        normalize_protocol_data(&data_type, &mut data);
        Self {
            r#type: "environment_data".into(),
            version: 1,
            device_id: device_id.into(),
            data_type,
            timestamp,
            sequence,
            data,
        }
    }

    pub fn normalize_for_transport(&mut self) {
        normalize_protocol_data(&self.data_type, &mut self.data);
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

fn normalize_protocol_data(data_type: &str, data: &mut Value) {
    if data_type != "bluetooth" || !data.is_object() {
        return;
    }
    if data.get("technology").and_then(Value::as_str) == Some("bluetooth") {
        data["technology"] = Value::String("unknown".into());
    }
    if let Some(nested) = data.get_mut("bluetooth") {
        if nested.get("technology").and_then(Value::as_str) == Some("bluetooth") {
            nested["technology"] = Value::String("unknown".into());
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
