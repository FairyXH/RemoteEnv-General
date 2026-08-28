use remote_env_platform_windows::wifi::{NativeWlanProvider, WlanProvider};

fn main() {
    match NativeWlanProvider::new().scan() {
        Ok(snapshot) => println!(
            "interfaces: {}, networks: {}, scan duration: {} ms",
            snapshot.interfaces,
            snapshot.networks.len(),
            snapshot.scan_duration_ms
        ),
        Err(error) => eprintln!("Wi-Fi scan failed: {error}"),
    }
}
