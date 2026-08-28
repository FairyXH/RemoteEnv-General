use super::{BluetoothError, BleScanner, ClassicBluetoothScanner, NativeBleScanner, NativeClassicBluetoothScanner};
use super::model::merge_observations;
use remote_env_core::bluetooth::{BluetoothObservation, BluetoothSnapshot};
use remote_env_core::collector::CollectorEvent;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub struct BluetoothScanResult { pub ble: Result<Vec<BluetoothObservation>, BluetoothError>, pub classic: Result<Vec<BluetoothObservation>, BluetoothError>, pub ble_available: bool, pub classic_available: bool }
pub trait BluetoothProvider: Send + Sync { fn scan(&self) -> BluetoothScanResult; }

pub struct BluetoothCollector<B = NativeBleScanner, C = NativeClassicBluetoothScanner> { ble: Arc<B>, classic: Arc<C>, allow_cross_transport_merge: bool }
impl<B: BleScanner, C: ClassicBluetoothScanner> BluetoothCollector<B, C> {
    pub fn new(ble: B, classic: C) -> Self { Self { ble: Arc::new(ble), classic: Arc::new(classic), allow_cross_transport_merge: false } }
    pub fn with_cross_transport_merge(mut self, enabled: bool) -> Self { self.allow_cross_transport_merge = enabled; self }
    pub fn scan_once(&self) -> Result<CollectorEvent, BluetoothError> {
        let started = Instant::now(); let result = self.scan();
        let mut observations = result.ble.unwrap_or_default(); observations.extend(result.classic.unwrap_or_default());
        let snapshot = BluetoothSnapshot { observations: merge_observations(observations, self.allow_cross_transport_merge), ble_available: result.ble_available, classic_available: result.classic_available, scan_duration_ms: started.elapsed().as_millis() as u64 };
        Ok(CollectorEvent { data_type: "bluetooth".into(), timestamp_ms: now_ms(), data: serde_json::to_value(snapshot).map_err(|e| BluetoothError::InvalidData(e.to_string()))? })
    }
}
impl<B: BleScanner, C: ClassicBluetoothScanner> BluetoothProvider for BluetoothCollector<B, C> {
    fn scan(&self) -> BluetoothScanResult { BluetoothScanResult { ble: self.ble.scan(), classic: self.classic.scan(), ble_available: self.ble.available(), classic_available: self.classic.available() } }
}
fn now_ms() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64 }

#[cfg(test)]
mod tests {
    use super::*;
    use remote_env_core::bluetooth::BluetoothTransport;
    struct MockBle(Result<Vec<BluetoothObservation>, BluetoothError>);
    impl BleScanner for MockBle { fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> { self.0.clone() } }
    struct MockClassic(Result<Vec<BluetoothObservation>, BluetoothError>);
    impl ClassicBluetoothScanner for MockClassic { fn scan(&self) -> Result<Vec<BluetoothObservation>, BluetoothError> { self.0.clone() } }
    fn obs(transport: BluetoothTransport, timestamp_ms: i64) -> BluetoothObservation { BluetoothObservation { address: "AA:BB:CC:DD:EE:01".into(), transport, name: None, rssi: None, service_uuids: vec![], manufacturer_data: vec![], service_data: vec![], connectable: None, class_of_device: None, appearance: None, tx_power: None, timestamp_ms } }
    #[test]
    fn emits_one_bluetooth_event_and_deduplicates_ble() { let collector = BluetoothCollector::new(MockBle(Ok(vec![obs(BluetoothTransport::Ble, 1), obs(BluetoothTransport::Ble, 2)])), MockClassic(Ok(vec![]))); let event = collector.scan_once().unwrap(); assert_eq!(event.data_type, "bluetooth"); assert_eq!(event.data["observations"].as_array().unwrap().len(), 1); }
    #[test]
    fn partial_failure_keeps_other_transport() { let collector = BluetoothCollector::new(MockBle(Err(BluetoothError::Unavailable("ble".into()))), MockClassic(Ok(vec![obs(BluetoothTransport::Classic, 1)]))); let event = collector.scan_once().unwrap(); assert_eq!(event.data["observations"][0]["transport"], "classic"); }
    #[test]
    fn explicit_cross_transport_merge_is_dual() { let collector = BluetoothCollector::new(MockBle(Ok(vec![obs(BluetoothTransport::Ble, 1)])), MockClassic(Ok(vec![obs(BluetoothTransport::Classic, 2)]))).with_cross_transport_merge(true); let event = collector.scan_once().unwrap(); assert_eq!(event.data["observations"][0]["transport"], "dual"); }
}