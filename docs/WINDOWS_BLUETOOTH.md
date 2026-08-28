# Windows Bluetooth Collector

## Status

Phase 2-B is **Complete**. The Windows platform crate contains one unified `BluetoothCollector` with independent BLE and Classic scanner boundaries. A scan produces one `CollectorEvent` with `data_type = "bluetooth"`; scanners do not allocate sequences, touch SQLite, or upload data. `RuntimeSupervisor::start_with_collectors` owns one Bluetooth periodic worker and routes its events through the existing sequence, target delivery, ServerWorker, and ACK path.

## Model and aggregation

`remote_env_core::bluetooth` defines `BluetoothObservation`, `BluetoothSnapshot`, `BluetoothTransport`, `ManufacturerData`, and `ServiceData`. `transport` is `ble`, `classic`, or `dual`. Windows addresses are normalized from the six-byte low portion of the native `u64` value to uppercase `AA:BB:CC:DD:EE:FF` text.

Repeated observations within one transport are deduplicated by address. The newest timestamp supplies scalar values; UUIDs are sorted/deduplicated and manufacturer/service entries are replaced by the newest value for the same key. Cross-transport merging is disabled by default because Windows address correlation between BLE and BR/EDR has not been proven for this product. Tests can enable it and verify `dual`.

BLE AD parsing covers flags/connectability, complete and shortened local name, 16-bit service UUIDs, 16-bit service data, manufacturer data, appearance, and Tx power. Truncated fields are ignored without panic. WinRT `DataSections()` retains every section as `source`, `ad_type`, and uppercase `data_hex`; advertisement and scan response are distinguished when reported by Windows. The pure parser test verifies an unknown type and exact bytes survive JSON serialization.

## Windows APIs

Classic discovery uses `BluetoothFindFirstRadio`, `BluetoothFindFirstDevice`, `BluetoothFindNextDevice`, and their close functions from `bluetoothapis.dll` through `windows-sys`. The native structure reliably supplies address, name, class of device, and discovery flags; RSSI and advertisement/service payloads are left unavailable. The synchronous inquiry must be isolated from the Runtime control thread when integrated.

The WinRT `Windows.Devices.Bluetooth.Advertisement.BluetoothLEAdvertisementWatcher` is implemented through the `windows` crate. It starts active scanning, extracts address, RSSI, local name, service UUIDs, manufacturer data, connectability, and Tx power in an event handler, sends observations through a bounded channel, then stops and unregisters the handler at the end of the scan window.

## Verification

Platform unit tests cover canonical address formatting, standard AD parsing, malformed/truncated input, BLE and Classic same-source deduplication, empty results, partial source failure, and explicit dual merge. The Windows probe is:

```text
cargo run -p remote-env-platform-windows --example windows_bluetooth_scan
```

Observed on the validation host: `BLE available: true`, `Classic Bluetooth available: true`, `unique Bluetooth device count: 5`, duration `5587 ms`. Both values come from a real WinRT BLE watcher plus native Classic inquiry scan.

## Completion

Phase 2-B is complete locally. The implementation includes the unified Collector, WinRT BLE watcher, native Classic inquiry, raw AD section retention, shared Runtime worker/config/status, single and multi-server fixtures, ACK isolation, disconnect recovery, dynamic enable/disable, shared UI status, and real Windows hardware validation. Real backend upload remains intentionally out of scope.