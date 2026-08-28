use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BluetoothTransport {
    Ble,
    Classic,
    Dual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManufacturerData {
    pub company_id: u16,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceData {
    pub uuid: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothObservation {
    pub address: String,
    pub transport: BluetoothTransport,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub service_uuids: Vec<String>,
    pub manufacturer_data: Vec<ManufacturerData>,
    pub service_data: Vec<ServiceData>,
    pub connectable: Option<bool>,
    pub class_of_device: Option<u32>,
    pub appearance: Option<u16>,
    pub tx_power: Option<i16>,
    pub timestamp_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothSnapshot {
    pub observations: Vec<BluetoothObservation>,
    pub ble_available: bool,
    pub classic_available: bool,
    pub scan_duration_ms: u64,
}
