use super::error::BluetoothError;
use super::model::format_bluetooth_address;
use remote_env_core::bluetooth::{
    BluetoothObservation, BluetoothTransport, RawAdvertisementSection,
};
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use windows::Devices::Bluetooth::Advertisement::{
    BluetoothLEAdvertisementReceivedEventArgs, BluetoothLEAdvertisementWatcher,
    BluetoothLEScanningMode,
};
use windows::Foundation::TypedEventHandler;
use windows::Storage::Streams::DataReader;
use windows::core::GUID;

#[derive(Debug, Default, Clone)]
pub struct BleAdvertisement {
    pub name: Option<String>,
    pub rssi: Option<i16>,
    pub service_uuids: Vec<String>,
    pub manufacturer_data: Vec<remote_env_core::bluetooth::ManufacturerData>,
    pub service_data: Vec<remote_env_core::bluetooth::ServiceData>,
    pub raw_advertisement_sections: Vec<RawAdvertisementSection>,
    pub connectable: Option<bool>,
    pub appearance: Option<u16>,
    pub tx_power: Option<i16>,
}

fn read_buffer(buffer: windows::Storage::Streams::IBuffer) -> windows::core::Result<Vec<u8>> {
    let reader = DataReader::FromBuffer(&buffer)?;
    let mut bytes = vec![0u8; reader.UnconsumedBufferLength()? as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(bytes)
}
fn guid_string(guid: GUID) -> String {
    let hex = format!("{:032X}", guid.to_u128());
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn parse_advertisement(raw: &[u8]) -> BleAdvertisement {
    let mut result = BleAdvertisement::default();
    let mut offset = 0;
    while offset < raw.len() {
        let length = raw[offset] as usize;
        offset += 1;
        if length == 0 {
            continue;
        }
        if offset + length > raw.len() {
            break;
        }
        let ad_type = raw[offset];
        let data = &raw[offset + 1..offset + length];
        match ad_type {
            0x08 | 0x09 => result.name = String::from_utf8(data.to_vec()).ok(),
            0x0A => result.tx_power = data.first().map(|value| *value as i8 as i16),
            0x16 if data.len() >= 2 => {
                result
                    .service_data
                    .push(remote_env_core::bluetooth::ServiceData {
                        uuid: format!("{:02X}{:02X}", data[1], data[0]),
                        data: data[2..].to_vec(),
                    })
            }
            0xFF if data.len() >= 2 => {
                result
                    .manufacturer_data
                    .push(remote_env_core::bluetooth::ManufacturerData {
                        company_id: u16::from_le_bytes([data[0], data[1]]),
                        data: data[2..].to_vec(),
                    })
            }
            0x19 if data.len() >= 2 => {
                result.appearance = Some(u16::from_le_bytes([data[0], data[1]]))
            }
            0x02 | 0x03 => {
                for chunk in data.chunks_exact(2) {
                    result
                        .service_uuids
                        .push(format!("{:02X}{:02X}", chunk[1], chunk[0]));
                }
            }
            0x01 if !data.is_empty() => result.connectable = Some(data[0] & 0x04 != 0),
            _ => result
                .raw_advertisement_sections
                .push(RawAdvertisementSection {
                    source: "advertisement".into(),
                    ad_type,
                    data_hex: data.iter().map(|byte| format!("{byte:02X}")).collect(),
                }),
        }
        offset += length;
    }
    result
}

pub trait BleScanner: Send + Sync {
    fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError>;
    fn available(&self) -> bool {
        true
    }
}
pub struct NativeBleScanner {
    scan_window: Duration,
}
impl NativeBleScanner {
    pub fn new() -> Self {
        Self {
            scan_window: Duration::from_secs(10),
        }
    }
}
impl Default for NativeBleScanner {
    fn default() -> Self {
        Self::new()
    }
}
impl BleScanner for NativeBleScanner {
    fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> {
        unsafe {
            windows::Win32::System::WinRT::RoInitialize(
                windows::Win32::System::WinRT::RO_INIT_MULTITHREADED,
            )
            .map_err(|e| BluetoothError::Unavailable(format!("WinRT 初始化失败: {e}")))?;
        }
        let (tx, rx) = channel();
        let watcher = BluetoothLEAdvertisementWatcher::new()
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        watcher
            .SetScanningMode(BluetoothLEScanningMode::Active)
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        watcher
            .SetAllowExtendedAdvertisements(true)
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        let callback = TypedEventHandler::<
            BluetoothLEAdvertisementWatcher,
            BluetoothLEAdvertisementReceivedEventArgs,
        >::new(move |_sender, args| {
            let Some(args) = args.as_ref() else {
                return Ok(());
            };
            let advertisement = args.Advertisement()?;
            let mut service_uuids = Vec::new();
            if let Ok(values) = advertisement.ServiceUuids() {
                for index in 0..values.Size()? {
                    if let Ok(guid) = values.GetAt(index) {
                        service_uuids.push(guid_string(guid));
                    }
                }
            }
            let mut manufacturer_data = Vec::new();
            if let Ok(values) = advertisement.ManufacturerData() {
                for index in 0..values.Size()? {
                    if let Ok(value) = values.GetAt(index) {
                        if let Ok(company_id) = value.CompanyId() {
                            let data = value
                                .Data()
                                .ok()
                                .and_then(|buffer| read_buffer(buffer).ok())
                                .unwrap_or_default();
                            manufacturer_data.push(remote_env_core::bluetooth::ManufacturerData {
                                company_id,
                                data,
                            });
                        }
                    }
                }
            }
            let source = if args.IsScanResponse().unwrap_or(false) {
                "scan_response"
            } else {
                "advertisement"
            };
            let mut raw_advertisement_sections = Vec::new();
            if let Ok(sections) = advertisement.DataSections() {
                for index in 0..sections.Size()? {
                    let Ok(section) = sections.GetAt(index) else {
                        continue;
                    };
                    let (Ok(ad_type), Ok(buffer)) = (section.DataType(), section.Data()) else {
                        continue;
                    };
                    let Ok(data) = read_buffer(buffer) else {
                        continue;
                    };
                    raw_advertisement_sections.push(RawAdvertisementSection {
                        source: source.into(),
                        ad_type,
                        data_hex: data.iter().map(|byte| format!("{byte:02X}")).collect(),
                    });
                }
            }
            let event = BluetoothObservation {
                address: format_bluetooth_address(args.BluetoothAddress()?),
                transport: BluetoothTransport::Ble,
                name: advertisement
                    .LocalName()
                    .ok()
                    .map(|value| value.to_string_lossy())
                    .filter(|value| !value.is_empty()),
                rssi: args.RawSignalStrengthInDBm().ok(),
                service_uuids,
                manufacturer_data,
                service_data: Vec::new(),
                raw_advertisement_sections,
                connectable: args.IsConnectable().ok(),
                class_of_device: None,
                appearance: None,
                tx_power: args
                    .TransmitPowerLevelInDBm()
                    .ok()
                    .and_then(|value| value.Value().ok()),
                timestamp_ms: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            };
            let _ = tx.send(event);
            Ok(())
        });
        let token = watcher
            .Received(&callback)
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        watcher
            .Start()
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        std::thread::sleep(self.scan_window);
        let status = watcher
            .Status()
            .map_err(|e| BluetoothError::Unavailable(format!("BLE watcher status failed: {e}")))?;
        if status == windows::Devices::Bluetooth::Advertisement::BluetoothLEAdvertisementWatcherStatus::Aborted {
            return Err(BluetoothError::Unavailable("BLE watcher aborted before receiving advertisements".into()));
        }
        let stop_result = watcher.Stop();
        let remove_result = watcher.RemoveReceived(token);
        stop_result
            .and(remove_result)
            .map_err(|e| BluetoothError::Unavailable(e.to_string()))?;
        Ok(rx.try_iter().collect())
    }
    fn available(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_standard_ad_fields_and_ignores_truncated_field() {
        let raw = [
            2, 0x01, 0x06, 5, 0x09, b'T', b'a', b'g', b'1', 3, 0x0A, 0xC3, 0x00, 5, 0xFF, 0x4C,
            0x00, 0x01, 0x02, 3, 0x19, 0x34, 0x12, 4, 0x16, 0xAA, 0xFE, 0x01, 5, 0x03, 0x0D, 0x18,
            0x0F, 0x18, 4, 0x20, 0xDE, 0xAD, 0xBE,
        ];
        let parsed = parse_advertisement(&raw);
        assert_eq!(parsed.name.as_deref(), Some("Tag1"));
        assert_eq!(parsed.tx_power, Some(-61));
        assert_eq!(parsed.appearance, Some(0x1234));
        assert_eq!(parsed.manufacturer_data[0].company_id, 0x004C);
        assert_eq!(parsed.service_data[0].uuid, "FEAA");
        assert_eq!(parsed.service_uuids, vec!["180D", "180F"]);
        assert_eq!(parsed.raw_advertisement_sections[0].ad_type, 0x20);
        assert_eq!(parsed.raw_advertisement_sections[0].data_hex, "DEADBE");
        let event = serde_json::json!({"observations": [{"raw_advertisement_sections": parsed.raw_advertisement_sections}]});
        assert_eq!(
            event["observations"][0]["raw_advertisement_sections"][0]["data_hex"],
            "DEADBE"
        );
    }
}
