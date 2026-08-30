use remote_env_platform_windows::bluetooth::{
    BluetoothCollector, NativeBleScanner, NativeClassicBluetoothScanner,
};

fn main() {
    let collector = BluetoothCollector::new(
        NativeBleScanner::new(),
        NativeClassicBluetoothScanner::new(),
    );
    let started = std::time::Instant::now();
    match collector.scan_once() {
        Ok(event) => {
            let devices = event.data["devices"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            let ble = devices
                .iter()
                .filter(|item| matches!(item["mode"].as_str(), Some("ble") | Some("dual")))
                .count();
            let classic = devices
                .iter()
                .filter(|item| matches!(item["mode"].as_str(), Some("classic") | Some("dual")))
                .count();
            println!("BLE observations: {ble}");
            println!("Classic observations: {classic}");
            println!("unique Bluetooth device count: {}", devices.len());
            println!("scan duration: {} ms", started.elapsed().as_millis());
            if let Some(first) = devices.first() {
                println!(
                    "first device: address={}, address_type={}, name={:?}, rssi={:?}, service_uuids={:?}, service_data_keys={:?}",
                    first["address"].as_str().unwrap_or_default(),
                    first["address_type"].as_str().unwrap_or_default(),
                    first["name"],
                    first["rssi"],
                    first["service_uuids"],
                    first["service_data"]
                        .as_object()
                        .map(|map| map.keys().collect::<Vec<_>>())
                );
            }
        }
        Err(error) => println!("Real hardware Bluetooth scan: NOT AVAILABLE ({error})"),
    }
}
