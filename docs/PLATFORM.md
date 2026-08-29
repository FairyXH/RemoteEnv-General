# Platform Matrix

| Platform | Shell | Collector status | Current boundary |
| --- | --- | --- | --- |
| Windows | Tauri desktop | Wi-Fi and unified BLE/Classic Bluetooth available | `remote-env-platform-windows` owns WLAN and Bluetooth adapters. |
| Android | Tauri mobile planned | Not implemented | independent adapter placeholder; future Tauri mobile/Kotlin bridge. |
| Linux | Tauri desktop later | Not implemented | placeholder for NetworkManager/BlueZ adapters. |
| macOS | Tauri desktop later | Not implemented | placeholder for CoreWLAN/CoreBluetooth adapters. |

## Integration boundary

Shared UI calls Tauri commands. Tauri owns desktop lifecycle and platform integration; Core owns cross-platform runtime, queue, protocol, and persistence. Android Framework integration is reserved for Kotlin/Java behind the Android/Tauri boundary and is not implemented in this phase.

Windows Release 使用 Tauri NSIS 配置，同时提供 Portable 目录包。发版输出统一收集到 `Release/Windows/`，用户数据仍由 Tauri `app_data_dir` 管理。

```text
Shared UI -> Tauri -> Windows integration
                 -> Android integration -> Kotlin/Java -> Rust Core
```

## Windows plan

1. Wi-Fi scanning uses tested Windows WLAN APIs behind `WifiCollector`.
2. Bluetooth uses one `BluetoothCollector` with WinRT BLE watcher and native Classic inquiry.
3. Normalize both sources into one `bluetooth` event and preserve unknown BLE AD sections.
4. Keep Bluetooth worker and status under the existing RuntimeSupervisor; no Bluetooth upload subsystem exists.

Platform crates may not expose native Windows/Android types across their collector result boundary.

Windows Phase 2 local runtime integration is verified through the shared CollectorEvent and target delivery path. The native Wi-Fi and unified Bluetooth probes have passed on the validation host; final packaged desktop UI operation was not completed because the available WebView2 UIA automation path timed out.
