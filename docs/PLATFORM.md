# Platform Matrix

| Platform | Shell | Collector status | Current boundary |
| --- | --- | --- | --- |
| Windows | Tauri desktop | Not implemented | `remote-env-platform-windows` will own WLAN, BLE, and Classic Bluetooth adapters. |
| Android | Tauri mobile planned | Not implemented | independent adapter placeholder; future Tauri mobile/Kotlin bridge. |
| Linux | Tauri desktop later | Not implemented | placeholder for NetworkManager/BlueZ adapters. |
| macOS | Tauri desktop later | Not implemented | placeholder for CoreWLAN/CoreBluetooth adapters. |

## Windows plan

1. Implement Wi-Fi scanning through tested Windows WLAN APIs behind `WifiCollector`.
2. Normalize SSID, BSSID, RSSI, channel/frequency/security and scanner metadata to core types.
3. Evaluate Rust BLE integration against Windows Runtime advertisement APIs with real fixtures.
4. Investigate Classic Bluetooth independently. BLE capability is not Classic Bluetooth capability; report `Unavailable` when no stable path exists.
5. Attach tested Tauri tray lifecycle only after a truthful runtime status exists.

Platform crates may not expose native Windows/Android types across their collector result boundary.
