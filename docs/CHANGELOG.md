# Changelog

## Unreleased

### Added

- Phase 0 architecture, platform, protocol, UI, development, WebSocket, context, and changelog documentation.
- Initial Git repository for an otherwise empty RemoteEnvCollector directory.

### Verification

- Shared React UI completed `npm install` and `npm run build` successfully.
- Cargo workspace validation is pending a usable Rust toolchain; rustup installation did not create Cargo on this host.

### Recorded

- RemoteEnvServer code-level upload ACK contract: `data_result`.
- Persistent monotonic sequence requirement per `(device_id, data_type)`.
- Tauri 2 + React shared UI and Rust 2024 workspace decision.
