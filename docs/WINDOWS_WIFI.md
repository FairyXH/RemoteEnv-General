# Windows Wi-Fi Collector

Phase 2-A uses the native Windows WLAN Client API through `windows-sys` in `crates/platform-windows`.

## Flow

`WlanOpenHandle` -> `WlanEnumInterfaces` -> `WlanScan` -> `WlanGetNetworkBssList` -> `WlanCloseHandle`.
The BSS list is used instead of treating the available-network/profile list as a complete nearby scan. A scan is asynchronous; the returned BSS data is the driver's current snapshot.

## Model and event

`WiFiObservation` normalizes SSID, raw SSID bytes (hex when not valid UTF-8), hidden state, BSSID, RSSI, link quality, channel, frequency, band, PHY, security privacy bit, and interface GUID. One scan creates one `CollectorEvent` with `data_type = "wifi"` and a `WiFiSnapshot` containing all observations.

SSID bytes are never lossy-decoded: valid UTF-8 is exposed as `ssid`, invalid bytes as `ssid_bytes_hex`, and zero length as hidden. BSSIDs use uppercase colon-separated hex. Duplicate `(interface_id, bssid)` records are removed; the same BSSID on different interfaces is retained.

## Limits

The provider enumerates every WLAN interface and continues if one interface's BSS query fails. Frequency is converted to MHz and channel only when the mapping is known. 2.4, 5, and 6 GHz are expressible; 6 GHz depends on Windows, adapter, and driver support. Security authentication/encryption and network type are unavailable until reliable IE parsing is added. Normal desktop users generally do not need Administrator privileges, but WLAN service, policy, and drivers can deny results.

## Runtime integration

The production desktop path passes `NativeWlanProvider` into `RuntimeSupervisor::start_with_collector`. The worker uses the configured `scan_interval_seconds`, skips work while `wifi_enabled` is false, isolates blocking WLAN calls, applies a 120-second timeout, emits status transitions, and stops with the Runtime. Configuration updates are forwarded without process restart.

The current test coverage includes `crates/core/tests/phase2a.rs`, which verifies a Wi-Fi snapshot event through Runtime, `upload_deliveries`, WebSocket auth/device-list/upload/ACK, completed status, and same-sequence recovery behavior. Hardware validation was rerun after enabling the adapter: `cargo run -p remote-env-platform-windows --example windows_wifi_scan` returned `interfaces: 1, networks: 10, scan duration: 1501 ms`. `netsh wlan show interfaces` reported the same USB adapter with Hardware On and Software On; `netsh wlan show networks mode=bssid` reported 6 visible SSID groups and multiple BSS entries. The count difference is expected because the collector counts BSS observations while `netsh` output groups them by SSID. Real backend Wi-Fi upload remains pending until credentials are provided through the environment.
