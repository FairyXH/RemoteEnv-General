use remote_env_platform_windows::wifi::{NativeWlanProvider, WlanProvider};

fn main() {
    match NativeWlanProvider::new().scan() {
        Ok(snapshot) => {
            println!(
                "interfaces: {}, networks: {}, scan duration: {} ms",
                snapshot.interfaces,
                snapshot.networks.len(),
                snapshot.scan_duration_ms
            );
            println!(
                "interface: {:?}, is_connected: {}, gateway: {:?}, dns_servers: {:?}, ip_address: {:?}",
                snapshot.interface,
                snapshot.is_connected,
                snapshot.gateway,
                snapshot.dns_servers,
                snapshot.ip_address
            );
            if let Some(first) = snapshot.networks.first() {
                println!(
                    "first network: ssid={:?}, bssid={}, rssi={:?}, signal_dbm={:?}, channel={:?}, frequency_mhz={:?}, band={:?}",
                    first.ssid,
                    first.bssid,
                    first.rssi,
                    first.signal_dbm,
                    first.channel,
                    first.frequency_mhz,
                    first.band
                );
            }
        }
        Err(error) => eprintln!("Wi-Fi scan failed: {error}"),
    }
}
