# RemoteEnvCollector

RemoteEnvCollector is a long-running cross-platform environment collector for RemoteEnvServer.

The first delivery targets Windows. The shared Tauri 2 UI is prepared for Android; Linux and macOS have isolated platform adapter crates but no collection implementation yet.

## Status

This repository currently contains Phase 0 scaffolding and protocol analysis. It does not yet scan Wi-Fi, BLE, or Classic Bluetooth. Read [docs/CONTEXT.md](docs/CONTEXT.md) before continuing work.

## Layout

- `crates/core`: platform-neutral models, configuration, collector contracts, queue contracts, and WebSocket protocol types.
- `crates/platform-*`: platform adapters. They must never leak OS-native types into `core`.
- `app/ui`: shared React UI.
- `app/desktop/src-tauri`: Tauri 2 desktop/mobile shell and Windows tray integration boundary.
- `docs`: architectural and handover documentation.

## Development

The verified toolchain requirements and current commands are maintained in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).
