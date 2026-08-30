use super::model::merge_observations;
use super::{
    BleScanner, BluetoothError, ClassicBluetoothScanner, NativeBleScanner,
    NativeClassicBluetoothScanner,
};
use remote_env_core::bluetooth::{BluetoothObservation, BluetoothSnapshot};
use remote_env_core::collector::CollectorEvent;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct BluetoothScanResult {
    pub ble: Result<Vec<BluetoothObservation>, BluetoothError>,
    pub classic: Result<Vec<BluetoothObservation>, BluetoothError>,
    pub ble_available: bool,
    pub classic_available: bool,
}
pub trait BluetoothProvider: Send + Sync {
    fn scan(&self) -> BluetoothScanResult;
}

pub struct BluetoothCollector<B = NativeBleScanner, C = NativeClassicBluetoothScanner> {
    ble: Arc<B>,
    classic: Arc<C>,
    allow_cross_transport_merge: bool,
    rolling: Arc<Mutex<Vec<BluetoothObservation>>>,
}
impl<B: BleScanner + 'static, C: ClassicBluetoothScanner + 'static> BluetoothCollector<B, C> {
    pub fn new(ble: B, classic: C) -> Self {
        Self {
            ble: Arc::new(ble),
            classic: Arc::new(classic),
            allow_cross_transport_merge: false,
            rolling: Arc::new(Mutex::new(Vec::new())),
        }
    }
    pub fn with_cross_transport_merge(mut self, enabled: bool) -> Self {
        self.allow_cross_transport_merge = enabled;
        self
    }
    pub fn scan_once(&self) -> Result<CollectorEvent, BluetoothError> {
        let started = Instant::now();
        let result = self.scan();
        let BluetoothScanResult {
            ble,
            classic,
            ble_available,
            classic_available,
        } = result;
        if ble.is_err() && classic.is_err() {
            return Err(BluetoothError::Unavailable(
                "BLE and Classic Bluetooth scanners both failed".into(),
            ));
        }
        let mut observations = ble.unwrap_or_default();
        observations.extend(classic.unwrap_or_default());
        let mut rolling = self.rolling.lock().map_err(|_| {
            BluetoothError::Unavailable("rolling Bluetooth snapshot unavailable".into())
        })?;
        rolling.extend(observations);
        let newest = rolling
            .iter()
            .map(|item| item.timestamp_ms)
            .max()
            .unwrap_or_else(now_ms);
        let cutoff = newest.saturating_sub(120_000);
        rolling.retain(|item| item.timestamp_ms >= cutoff);
        let observations = merge_observations(rolling.clone(), self.allow_cross_transport_merge);
        *rolling = observations.clone();
        let snapshot = BluetoothSnapshot {
            observations,
            ble_available,
            classic_available,
            scan_duration_ms: started.elapsed().as_millis() as u64,
        };
        let devices: Vec<serde_json::Value> = snapshot
            .observations
            .iter()
            .map(|observation| {
                let mode = match observation.transport {
                    remote_env_core::bluetooth::BluetoothTransport::Ble => "ble",
                    remote_env_core::bluetooth::BluetoothTransport::Classic => "classic",
                    remote_env_core::bluetooth::BluetoothTransport::Dual => "dual",
                };
                let technology = match observation.transport {
                    remote_env_core::bluetooth::BluetoothTransport::Ble => "ble",
                    remote_env_core::bluetooth::BluetoothTransport::Classic => "bluetooth_classic",
                    remote_env_core::bluetooth::BluetoothTransport::Dual => "unknown",
                };
                serde_json::json!({
                    "address": observation.address,
                    "address_type": "unknown",
                    "name": observation.name,
                    "rssi": observation.rssi,
                    "technology": technology,
                    "mode": mode,

                    "is_connected": Value::Null,
                    "is_paired": Value::Null,
                    "classOfDevice": observation.class_of_device,
                    "tx_power": observation.tx_power,
                    "manufacturer_id": observation.manufacturer_data.first().map(|item| item.company_id),
                    "manufacturer_data": observation.manufacturer_data.first().map(|item| base64_encode(&item.data)),
                    "service_uuids": observation.service_uuids,
                    "service_data": observation.service_data.iter().map(|item| (item.uuid.clone(), base64_encode(&item.data))).collect::<std::collections::HashMap<_, _>>(),
                    "rawAdvertisementSections": observation.raw_advertisement_sections,
                    "rawHex": observation.raw_advertisement.as_ref().map(|value| hex_encode(value)),
                    "rawLength": observation.raw_advertisement.as_ref().map(Vec::len),
                    "raw": observation.raw_advertisement.as_ref().map(|value| base64_encode(value)),
                    "connectable": observation.connectable,
                    "appearance": observation.appearance,
                    "timestamp": observation.timestamp_ms,
                })
            })
            .collect();
        let data = serde_json::json!({
            "scan_started_at": now_ms().saturating_sub(snapshot.scan_duration_ms as i64),
            "scan_finished_at": now_ms(),
            "technology": if snapshot.ble_available && !snapshot.classic_available {
                "ble"
            } else if snapshot.classic_available && !snapshot.ble_available {
                "bluetooth_classic"
            } else {
                "unknown"
            },
            "is_enabled": snapshot.ble_available || snapshot.classic_available,
            "devices": devices,
        });
        Ok(CollectorEvent {
            data_type: "bluetooth".into(),
            timestamp_ms: now_ms(),
            data,
        })
    }
}
impl<B: BleScanner + 'static, C: ClassicBluetoothScanner + 'static> BluetoothProvider
    for BluetoothCollector<B, C>
{
    fn scan(&self) -> BluetoothScanResult {
        let ble = Arc::clone(&self.ble);
        let classic = Arc::clone(&self.classic);
        let ble_thread = std::thread::spawn(move || ble.scan());
        let classic_thread = std::thread::spawn(move || classic.scan());
        BluetoothScanResult {
            ble: ble_thread.join().unwrap_or_else(|_| {
                Err(BluetoothError::Unavailable(
                    "BLE scanner thread panicked".into(),
                ))
            }),
            classic: classic_thread.join().unwrap_or_else(|_| {
                Err(BluetoothError::Unavailable(
                    "Classic Bluetooth scanner thread panicked".into(),
                ))
            }),
            ble_available: self.ble.available(),
            classic_available: self.classic.available(),
        }
    }
}
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn base64_encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let value = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or_default() as u32) << 8)
            | chunk.get(2).copied().unwrap_or_default() as u32;
        output.push(TABLE[((value >> 18) & 0x3f) as usize] as char);
        output.push(TABLE[((value >> 12) & 0x3f) as usize] as char);
        output.push(if chunk.len() > 1 {
            TABLE[((value >> 6) & 0x3f) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            TABLE[(value & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use remote_env_core::bluetooth::BluetoothTransport;
    struct MockBle(Result<Vec<BluetoothObservation>, BluetoothError>);
    impl BleScanner for MockBle {
        fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> {
            self.0.clone()
        }
    }
    struct MockClassic(Result<Vec<BluetoothObservation>, BluetoothError>);
    impl ClassicBluetoothScanner for MockClassic {
        fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> {
            self.0.clone()
        }
    }
    fn obs(transport: BluetoothTransport, timestamp_ms: i64) -> BluetoothObservation {
        BluetoothObservation {
            address: "AA:BB:CC:DD:EE:01".into(),
            transport,
            name: None,
            rssi: None,
            service_uuids: vec![],
            manufacturer_data: vec![],
            service_data: vec![],
            raw_advertisement_sections: vec![],
            raw_advertisement: None,
            connectable: None,
            class_of_device: None,
            appearance: None,
            tx_power: None,
            timestamp_ms,
        }
    }
    #[test]
    fn emits_one_bluetooth_event_and_deduplicates_ble() {
        let collector = BluetoothCollector::new(
            MockBle(Ok(vec![
                obs(BluetoothTransport::Ble, 1),
                obs(BluetoothTransport::Ble, 2),
            ])),
            MockClassic(Ok(vec![])),
        );
        let event = collector.scan_once().unwrap();
        assert_eq!(event.data_type, "bluetooth");
        assert_eq!(event.data["devices"].as_array().unwrap().len(), 1);
    }
    #[test]
    fn partial_failure_keeps_other_transport() {
        let collector = BluetoothCollector::new(
            MockBle(Err(BluetoothError::Unavailable("ble".into()))),
            MockClassic(Ok(vec![obs(BluetoothTransport::Classic, 1)])),
        );
        let event = collector.scan_once().unwrap();
        assert_eq!(event.data["devices"][0]["mode"], "classic");
    }
    #[test]
    fn classic_failure_keeps_ble_transport() {
        let collector = BluetoothCollector::new(
            MockBle(Ok(vec![obs(BluetoothTransport::Ble, 1)])),
            MockClassic(Err(BluetoothError::Unavailable("classic".into()))),
        );
        let event = collector.scan_once().unwrap();
        assert_eq!(event.data["devices"][0]["mode"], "ble");
    }
    #[test]
    fn both_sources_failed_returns_error() {
        let collector = BluetoothCollector::new(
            MockBle(Err(BluetoothError::Unavailable("ble".into()))),
            MockClassic(Err(BluetoothError::Unavailable("classic".into()))),
        );
        assert!(collector.scan_once().is_err());
    }
    #[test]
    fn uploads_complete_raw_record_as_base64_with_vir_env_tester_keys() {
        let mut observation = obs(BluetoothTransport::Ble, 1);
        observation.raw_advertisement = Some(vec![0x02, 0x01, 0x06, 0x03, 0xFF, 0x4C, 0x00]);
        let collector =
            BluetoothCollector::new(MockBle(Ok(vec![observation])), MockClassic(Ok(vec![])));
        let device = &collector.scan_once().unwrap().data["devices"][0];
        assert_eq!(device["rawHex"], "02010603FF4C00");
        assert_eq!(device["rawLength"], 7);
        assert_eq!(device["raw"], "AgEGA/9MAA==");
    }

    #[test]
    fn explicit_cross_transport_merge_is_dual() {
        let collector = BluetoothCollector::new(
            MockBle(Ok(vec![obs(BluetoothTransport::Ble, 1)])),
            MockClassic(Ok(vec![obs(BluetoothTransport::Classic, 2)])),
        )
        .with_cross_transport_merge(true);
        let event = collector.scan_once().unwrap();
        assert_eq!(event.data["devices"][0]["mode"], "dual");
    }
}
