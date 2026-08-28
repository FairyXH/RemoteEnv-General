use remote_env_core::bluetooth::{BluetoothObservation, ManufacturerData, ServiceData};
use super::error::BluetoothError;

#[derive(Debug, Default, Clone)]
pub struct BleAdvertisement {
    pub name: Option<String>, pub rssi: Option<i16>, pub service_uuids: Vec<String>,
    pub manufacturer_data: Vec<ManufacturerData>, pub service_data: Vec<ServiceData>,
    pub connectable: Option<bool>, pub appearance: Option<u16>, pub tx_power: Option<i16>,
}

pub fn parse_advertisement(raw: &[u8]) -> BleAdvertisement {
    let mut result = BleAdvertisement::default(); let mut offset = 0;
    while offset < raw.len() {
        let length = raw[offset] as usize; offset += 1;
        if length == 0 { continue; } if offset + length > raw.len() { break; }
        let ad_type = raw[offset]; let data = &raw[offset + 1..offset + length];
        match ad_type {
            0x08 | 0x09 => result.name = String::from_utf8(data.to_vec()).ok(),
            0x0A => result.tx_power = data.first().map(|value| *value as i8 as i16),
            0x16 if data.len() >= 2 => result.service_data.push(ServiceData { uuid: format!("{:02X}{:02X}", data[1], data[0]), data: data[2..].to_vec() }),
            0xFF if data.len() >= 2 => result.manufacturer_data.push(ManufacturerData { company_id: u16::from_le_bytes([data[0], data[1]]), data: data[2..].to_vec() }),
            0x19 if data.len() >= 2 => result.appearance = Some(u16::from_le_bytes([data[0], data[1]])),
            0x02 | 0x03 => for chunk in data.chunks_exact(2) { result.service_uuids.push(format!("{:02X}{:02X}", chunk[1], chunk[0])); },
            0x01 if !data.is_empty() => result.connectable = Some(data[0] & 0x04 != 0),
            _ => {}
        }
        offset += length;
    }
    result
}

pub trait BleScanner: Send + Sync { fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError>; fn available(&self) -> bool { true } }
pub struct NativeBleScanner;
impl NativeBleScanner { pub fn new() -> Self { Self } }
impl Default for NativeBleScanner { fn default() -> Self { Self::new() } }
impl BleScanner for NativeBleScanner {
    fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> { Err(BluetoothError::Unavailable("WinRT advertisement watcher unavailable in this build".into())) }
    fn available(&self) -> bool { false }
}