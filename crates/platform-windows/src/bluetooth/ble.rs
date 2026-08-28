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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_standard_ad_fields_and_ignores_truncated_field() {
        let raw = [2, 0x01, 0x06, 5, 0x09, b'T', b'a', b'g', b'1', 3, 0x0A, 0xC3, 0x00, 5, 0xFF, 0x4C, 0x00, 0x01, 0x02, 3, 0x19, 0x34, 0x12, 4, 0x16, 0xAA, 0xFE, 0x01, 5, 0x03, 0x0D, 0x18, 0x0F, 0x18, 8, 0xFF, 0x01];
        let parsed = parse_advertisement(&raw);
        assert_eq!(parsed.name.as_deref(), Some("Tag1"));
        assert_eq!(parsed.tx_power, Some(-61));
        assert_eq!(parsed.appearance, Some(0x1234));
        assert_eq!(parsed.manufacturer_data[0].company_id, 0x004C);
        assert_eq!(parsed.service_data[0].uuid, "FEAA");
        assert_eq!(parsed.service_uuids, vec!["180D", "180F"]);
    }
}