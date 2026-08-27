# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The compact shell includes connection state, collector states/counts, queue depth, accepted-upload statistics, and controls disabled until a real runtime exists. It shows honest `Not started`/`Not implemented` values, never simulated hardware data.

## Tray

Tauri desktop owns the tray boundary. Planned menu: Open, Start/Pause, connection summary, Exit. Close-to-tray is not claimed until lifecycle and exit cleanup are tested.
