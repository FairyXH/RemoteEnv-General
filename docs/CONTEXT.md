# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current state

Phase 0 scaffolding. The client directory was empty before this work and is now a Git repository. No existing client files were removed.

## Completed

- Read RemoteEnvServer API docs and active WS implementation.
- Chosen Rust 2024 workspace plus Tauri 2 and shared React + TypeScript UI.
- Defined isolated Windows, Android, Linux, and macOS crates.
- Documented exact `data_result` ACK and persistent sequence constraints.

## Not implemented

- WS/auth/heartbeat/reconnect/queue/config persistence.
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
- Rust/Cargo/rustup, Flutter, and Dart absent from PATH.
- `winget` exists but community source queries failed; Rust bootstrap may require official installer.

## Important paths

- Client: `D:\Files\Develop\Cross-Platform\RemoteEnvCollector`
- Server API: `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\docs\api.md`
- Server WS runtime: `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\remote_env_server\bus.py`

## Verification status

- `npm install` and `npm run build` completed successfully on 2026-08-27.
- Production dependency audit returned `0` vulnerabilities; development dependency audit reported two advisories and needs a deliberate dependency-update review later.
- Cargo verification is blocked because Rust/Cargo/rustup are absent. The official rustup installer was downloaded and invoked twice, but it exited without creating `%USERPROFILE%\.cargo`; do not claim Cargo verification until this machine has a usable Rust toolchain.

## Next

1. Repair the Rust toolchain installation and run `cargo fmt --check`, `cargo check --workspace`, and `cargo test --workspace`.
2. Design durable configuration and per-data-type sequence persistence before sender implementation.
3. Build protocol tests against a local server fixture before physical collection.
