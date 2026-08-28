# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The compact shell includes live Core connection state, collector states/counts, queue depth, accepted-upload statistics, and a Start Runtime command. In browser preview or before Tauri is running, it honestly falls back to unavailable/offline values. It never presents mock events as hardware scans.

## Tray

Tauri desktop owns the tray boundary. The Phase 1.5 shell does not yet register a tray menu; close-to-tray and tray-triggered graceful shutdown remain deferred until the lifecycle is tested.
