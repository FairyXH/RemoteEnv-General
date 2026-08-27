use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollectorEvent {
    pub data_type: String,
    pub timestamp_ms: i64,
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectorKind { Wifi, Ble, ClassicBluetooth }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapabilityState { Available, Unavailable, NotImplemented }

pub trait Collector: Send + Sync {
    fn kind(&self) -> CollectorKind;
    fn capability(&self) -> CapabilityState;
}
