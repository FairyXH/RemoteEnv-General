mod ble;
mod classic;
mod collector;
mod error;
mod model;

pub use ble::{parse_advertisement, BleAdvertisement, BleScanner, NativeBleScanner};
pub use classic::{ClassicBluetoothScanner, NativeClassicBluetoothScanner};
pub use collector::{BluetoothCollector, BluetoothProvider, BluetoothScanResult};
pub use error::BluetoothError;
pub use model::{format_bluetooth_address, merge_observations};