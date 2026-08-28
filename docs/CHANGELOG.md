# Changelog

## Unreleased

### Phase 2-A (partial)

- Added the Windows WLAN BSS collector foundation using `windows-sys`, including normalized Wi-Fi snapshots, mock-provider tests, and a manual scan example.
- Added a Tauri `scan_wifi` command; upload remains exclusively in the existing Runtime/Dispatcher path.

### Phase 1.75-C

- RuntimeSupervisor now owns the collector event channel and persists each event directly into `upload_deliveries`; the legacy `upload_queue` is no longer the Runtime upload path.
- DispatcherSupervisor starts one independently stopped worker per selected server and cancels removed targets.
- ServerWorker exposes per-target state, strict target-scoped ACKs, target-local recovery, capped reconnect, heartbeat, and permanent authentication blocking.
- Runtime status and the shared UI expose per-server state and real delivery counts.
- Local multi-server disconnect/recovery fixture coverage and standalone Python real-backend smoke verification passed; Phase 1.75-C is Complete.
- Added `phase175c.rs` with two independent local WebSocket listeners covering RuntimeSupervisor multi-target delivery, reconnect/resend, target removal, and permanent auth failure.
- Extended Phase 1.75-C coverage with profile replacement, Single-to-Multi transition, rate-limit isolation, heartbeat/auth observation, target delivery-state assertions, and worker heartbeat timeout handling.
- Corrected-endpoint standalone Python smoke completed TLS/auth/auth_result/device_list/heartbeat/pong/one marked environment_data/data_result ACK successfully; credentials were environment-only and cleaned up afterward.


- Added target-scoped `UploadDispatcher` groundwork and additive SQLite delivery storage for Single/Multi target resolution.
- The Rust environment-only smoke-test skeleton remains ignored by default; the real backend protocol chain was verified independently with Python.
- Added Windows WLAN research notes for Phase 2.
- Phase 1.75-B groundwork is superseded by the completed Phase 1.75-C live runtime integration.

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
