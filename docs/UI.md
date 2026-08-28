# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The UI reads `get_runtime_status` and renders the real `servers` list, including each profile's connection state and target delivery counts. Wi-Fi and Bluetooth status are sourced from the Runtime status model. Bluetooth is now represented by one shared Runtime status card; the UI does not claim separate BLE/Classic upload states. Detailed payloads retain `transport` (`ble`, `classic`, `dual`) and raw AD sections, while the main status displays the aggregate Bluetooth worker.

## Tray

Tauri desktop owns the tray boundary. The Phase 1.5 shell does not yet register a tray menu; close-to-tray and tray-triggered graceful shutdown remain deferred until the lifecycle is tested.
