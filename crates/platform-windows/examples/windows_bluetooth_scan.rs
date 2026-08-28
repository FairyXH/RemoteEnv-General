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
            let observations = event.data["observations"].as_array().map_or(0, Vec::len);
            println!("BLE available: {}", event.data["ble_available"]);
            println!(
                "Classic Bluetooth available: {}",
                event.data["classic_available"]
            );
            println!("unique Bluetooth device count: {observations}");
            println!("scan duration: {} ms", started.elapsed().as_millis());
        }
        Err(error) => println!("Real hardware Bluetooth scan: NOT AVAILABLE ({error})"),
    }
}
