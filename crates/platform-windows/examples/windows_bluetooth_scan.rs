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
            let observations = event.data["observations"].as_array().cloned().unwrap_or_default();
            let ble = observations
                .iter()
                .filter(|item| matches!(item["transport"].as_str(), Some("ble") | Some("dual")))
                .count();
            let classic = observations
                .iter()
                .filter(|item| matches!(item["transport"].as_str(), Some("classic") | Some("dual")))
                .count();
            println!("BLE observations: {ble}");
            println!("Classic observations: {classic}");
            println!("unique Bluetooth device count: {}", observations.len());
            println!("scan duration: {} ms", started.elapsed().as_millis());
        }
        Err(error) => println!("Real hardware Bluetooth scan: NOT AVAILABLE ({error})"),
    }
}
