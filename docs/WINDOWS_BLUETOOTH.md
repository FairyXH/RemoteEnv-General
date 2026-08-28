# Windows Bluetooth Collector

## Status

Phase 2-B is **Partial**. The Windows platform crate now contains one unified `BluetoothCollector` with independent BLE and Classic scanner boundaries. A scan produces one `CollectorEvent` with `data_type = "bluetooth"`; scanners do not allocate sequences, touch SQLite, or upload data.

## Model and aggregation

`remote_env_core::bluetooth` defines `BluetoothObservation`, `BluetoothSnapshot`, `BluetoothTransport`, `ManufacturerData`, and `ServiceData`. `transport` is `ble`, `classic`, or `dual`. Windows addresses are normalized from the six-byte low portion of the native `u64` value to uppercase `AA:BB:CC:DD:EE:FF` text.

Repeated observations within one transport are deduplicated by address. The newest timestamp supplies scalar values; UUIDs are sorted/deduplicated and manufacturer/service entries are replaced by the newest value for the same key. Cross-transport merging is disabled by default because Windows address correlation between BLE and BR/EDR has not been proven for this product. Tests can enable it and verify `dual`.

BLE AD parsing covers flags/connectability, complete and shortened local name, 16-bit service UUIDs, 16-bit service data, manufacturer data, appearance, and Tx power. Truncated fields are ignored without panic. Unknown AD types are not yet retained as raw sections and remain a known limitation.

## Windows APIs

Classic discovery uses `BluetoothFindFirstRadio`, `BluetoothFindFirstDevice`, `BluetoothFindNextDevice`, and their close functions from `bluetoothapis.dll` through `windows-sys`. The native structure reliably supplies address, name, class of device, and discovery flags; RSSI and advertisement/service payloads are left unavailable. The synchronous inquiry must be isolated from the Runtime control thread when integrated.

The intended BLE implementation is WinRT `Windows.Devices.Bluetooth.Advertisement.BluetoothLEAdvertisementWatcher`. The `windows` binding dependency is present, but the watcher event lifecycle is not connected yet. The current native BLE scanner therefore reports unavailable rather than producing synthetic data.

## Verification

Platform unit tests cover canonical address formatting, standard AD parsing, malformed/truncated input, BLE and Classic same-source deduplication, empty results, partial source failure, and explicit dual merge. The Windows probe is:

```text
cargo run -p remote-env-platform-windows --example windows_bluetooth_scan
```

Observed on the validation host: `BLE available: false`, `Classic Bluetooth available: true`, `unique Bluetooth device count: 4`, duration `2567 ms`. This is real Classic inquiry output. No BLE hardware result is claimed.

## Remaining Phase 2-B work

Connect the stop-aware WinRT watcher, add one Runtime-owned Bluetooth worker using `bluetooth_enabled` and `scan_interval_seconds`, publish per-source runtime status, add `crates/core/tests/phase2b.rs` single/multi-server/recovery fixtures, expose shared UI status, preserve unknown raw AD sections, and rerun UI build plus all gates. Real backend tests and credentials remain out of scope.