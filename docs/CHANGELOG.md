# Changelog

## Unreleased

### Phase 1

- Added SQLite-backed configuration, stable identity, durable per-data-type sequences, bounded persistent upload queue, ACK matching, heartbeat monitoring, explicit connection states, and capped infinite reconnect backoff.
- Added Core tests for persistence, queue recovery, serialization, ACK matching, state classification, heartbeat, sequence recovery, and backoff.
- Real Wi-Fi, BLE, and Classic Bluetooth collection remain `Not implemented`.

### Added

- Phase 0 architecture, platform, protocol, UI, development, WebSocket, context, and changelog documentation.
- Initial Git repository for an otherwise empty RemoteEnvCollector directory.

### Verification

- Shared React UI completed `npm install` and `npm run build` successfully.
- Core Cargo validation passes with Rust `1.98.0`; full workspace validation is blocked by missing `app/desktop/src-tauri/icons/icon.ico`.

### Recorded

- RemoteEnvServer code-level upload ACK contract: `data_result`.
- Persistent monotonic sequence requirement per `(device_id, data_type)`.
- Tauri 2 + React shared UI and Rust 2024 workspace decision.
