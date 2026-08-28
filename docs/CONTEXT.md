# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current state

Phase 1 implementation is partial: Core persistence, queue, protocol handling, and a single WebSocket lifecycle pass are implemented. A long-running reconnecting runtime loop and Tauri composition remain. Real platform scanners remain intentionally unimplemented.

## Completed

- Read RemoteEnvServer API docs and active WS implementation.
- Chosen Rust 2024 workspace plus Tauri 2 and shared React + TypeScript UI.
- Defined isolated Windows, Android, Linux, and macOS crates.
- Documented exact `data_result` ACK and persistent sequence constraints.

## Not implemented

- Tauri runtime composition and UI status commands.
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

## Phase 1 status

- Configuration, stable identity, SQLite-backed sequences, bounded durable queue, ACK matching, heartbeat monitoring, explicit states, and capped infinite retry backoff are implemented.
- `cargo fmt --check`, `cargo check -p remote-env-core`, and `cargo test -p remote-env-core` pass. Full workspace validation is blocked by missing Tauri `icons/icon.ico`.

## Next

1. Add Tauri runtime composition and status snapshots.
2. Add a MockCollector integration harness against a local WebSocket fixture.
3. Phase 2 — implement Windows Wi-Fi collection without changing Core queue/transport contracts.
