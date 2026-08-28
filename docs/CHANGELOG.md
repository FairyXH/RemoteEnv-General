# Changelog

## Unreleased

### Phase 1.5

- Added long-lived Core `RuntimeSupervisor` with bounded event channel, dedicated Tokio worker, reconnect loop, queue recovery, and Tauri lifecycle commands.
- Added local WebSocket fixture coverage for authentication, device list, upload, exact ACK, and disconnect.
- UI now reads `get_runtime_status`; tray and push-based Tauri events remain deferred.
- Real platform collectors remain `Not implemented`.

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
