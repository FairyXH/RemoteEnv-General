# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current state

Phase 2-A implementation is partial: Windows WLAN BSS scanning, normalized observations, mock collector tests, a manual scan example, Runtime-owned periodic scheduling, and a successful real Windows WLAN probe are implemented. Full Runtime integration coverage and backend upload evidence remain.

## Completed

- Read RemoteEnvServer API docs and active WS implementation.
- Chosen Rust 2024 workspace plus Tauri 2 and shared React + TypeScript UI.
- Defined isolated Windows, Android, Linux, and macOS crates.
- Documented exact `data_result` ACK and persistent sequence constraints.

## Not implemented

- Full Tauri tray lifecycle and push-based UI status events.
- Wi-Fi, BLE, Classic Bluetooth.
- Windows tray behavior, Android generated project, Linux/macOS collection.
- Hardware integration tests.

## Protocol constraints

- Collector auth is first WS message and requires `device` metadata.
- Server application heartbeat supports `ping` -> `pong` and timestamped `heartbeat` -> `pong`.
- ACK matches device/data type/sequence.
- Sequences must increase across client restarts.
- Process `error` immediately; do not wait only for ACK timeout.

## Toolchain inspection

- Node `v24.14.0`, npm `11.9.0`, Git `2.55.0.windows.1` installed.
- Rust `1.98.0`, Cargo `1.98.0`, and rustup stable MSVC are available.
- `winget` exists but community source queries failed; Rust bootstrap may require official installer.

## Important paths

- Client: `D:\Files\Develop\Cross-Platform\RemoteEnvCollector`
- Server API: `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\docs\api.md`
- Server WS runtime: `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\remote_env_server\bus.py`

## Phase 2-A status

Status: Partial. Implemented `windows-sys` WLAN bindings, BSS snapshots, normalization, mock tests, Tauri command, Runtime-owned periodic scheduling, dynamic configuration, blocking isolation, timeout, status propagation, and UI status metrics. Runtime scheduling tests pass. Real Windows probe succeeded after enabling Software Radio: 1 interface, 10 BSS observations, 1501 ms; `netsh` reported Software On and 6 visible SSID groups with multiple BSS entries. Dedicated Runtime upload fixture and real Wi-Fi backend upload remain unverified.

Next step: add the dedicated Runtime-to-`upload_deliveries` Wi-Fi fixture and run an environment-only real backend Wi-Fi upload test. Hardware proof is now available when the adapter is enabled.

## Phase 1.75-C status

Status: Complete. Runtime wiring is present: `RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker(s)`, target delivery persistence is used for new events, and per-server status is exposed to UI. Windows Wi-Fi, BLE, and Classic Bluetooth remain `Not implemented` and are reserved for Phase 2.

Local dual-server runtime coverage now verifies independent A/B readiness, same-envelope delivery, recovery, Single A -> Single B switching, and authentication blocking. The full Phase 1.75-C local checklist is covered by `phase175c.rs`; the phase is complete.
The fixture has since added explicit A/B delivery-state assertions, heartbeat/auth-frame observation, Single A -> Multi A+B, Multi A+B -> Single B, profile removal cancellation, profile URL/token replacement, rate-limit isolation, and missing-pong reconnect coverage. Real backend verification: PASS via a standalone Python WebSocket client using the corrected endpoint. Verified TLS WebSocket connection, auth, successful auth_result, device_list (7 devices), heartbeat/pong, one marked environment_data upload, and matching data_result ACK. Credentials were supplied only through process environment variables and were not stored.
- Configuration, stable identity, SQLite-backed sequences, bounded durable queue, ACK matching, heartbeat monitoring, explicit states, and capped infinite retry backoff are implemented.
- `cargo fmt --check`, `cargo check --workspace`, and `cargo test --workspace` pass. `npm run build` passes.

## Phase 1.5 status

- Tauri owns `AppState`, startup commands, stop commands, and status queries; Core owns RuntimeSupervisor and all transport/state logic.
- RuntimeSupervisor uses a bounded Tokio event channel, a dedicated Tokio runtime thread, persistent queue recovery, and infinite reconnect through WebSocketManager.
- Mock events enter through `submit_test_event`; they are explicitly marked `mock` and no real scanner is claimed.
- Fixture coverage validates auth, device list, upload, exact ACK, and connection close behavior.
- UI reads `get_runtime_status`; current refresh is a low-rate fallback until Tauri event push is added.
- Tray is not implemented yet.

## Phase 1.75-B status

- Status: Complete as superseded by the Phase 1.75-C runtime integration.
- Latest dispatcher commit: `ff1ab7c feat: add target scoped upload dispatcher`; module export follow-up: `e6c08dd chore: export upload dispatcher module`.
- `ServerProfile`, `ServerMode`, immutable target resolution, and global sequence semantics are implemented.
- `UploadDispatcher` now persists target-scoped delivery rows and exposes claim, exact ACK, recovery, block, cancel, and per-target statistics operations.
- SQLite migration is additive: existing `upload_queue` and sequence data are preserved; `upload_deliveries` is created if absent.
- Live per-server WebSocket supervisors, dispatcher wiring in `RuntimeSupervisor`, independent heartbeat/reconnect, and multi-server end-to-end fixture are implemented and verified by Phase 1.75-C.
- No credentials are stored in the repository. The next implementation phase is Windows Collector and is intentionally not started here.

