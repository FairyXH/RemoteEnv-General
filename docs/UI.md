# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The UI reads `get_runtime_status` and renders the real `servers` list, including each profile's connection state and target delivery counts. Collector capabilities remain `Not implemented`; no synthetic scan data is shown.

## Tray

Tauri desktop owns the tray boundary. The Phase 1.5 shell does not yet register a tray menu; close-to-tray and tray-triggered graceful shutdown remain deferred until the lifecycle is tested.
