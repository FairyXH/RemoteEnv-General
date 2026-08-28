# Changelog

## Unreleased

### Phase 1.75-C

- RuntimeSupervisor now owns the collector event channel and persists each event directly into `upload_deliveries`; the legacy `upload_queue` is no longer the Runtime upload path.
- DispatcherSupervisor starts one independently stopped worker per selected server and cancels removed targets.
- ServerWorker exposes per-target state, strict target-scoped ACKs, target-local recovery, capped reconnect, heartbeat, and permanent authentication blocking.
- Runtime status and the shared UI expose per-server state and real delivery counts.
- Local multi-server disconnect/recovery fixture coverage and real backend smoke verification remain pending; phase remains Partial.

- Added target-scoped `UploadDispatcher` groundwork and additive SQLite delivery storage for Single/Multi target resolution.
- Added explicit environment-only real backend smoke-test skeleton; it remains ignored by default and was not executed in this phase.
- Added Windows WLAN research notes for Phase 2.
- Phase 1.75-B remains partial: live per-server dispatch and reconnect fixtures are next.

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
