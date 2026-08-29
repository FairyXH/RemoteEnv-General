# Development

## Required toolchain

- Rust stable `1.85` or later via rustup.
- Node.js 22 or later and npm for the shared UI.
- Microsoft C++ Build Tools and WebView2 for Windows Tauri builds.
- Android Studio, Android SDK/NDK, and configured JDK for the future Android shell.

The workspace declares Rust 2024 edition and MSRV `1.85`.

## Commands

```powershell
cargo fmt --check
cargo check --workspace
cargo test --workspace

Set-Location app/ui
npm ci
npm run build

Set-Location ../desktop
npm run tauri dev
```

`cargo check --workspace` includes the Tauri crate. Android is intentionally outside the first Windows validation gate.

正式 Windows 发版使用仓库根目录的 `scripts/build-release.ps1`，输出只认 `Release/Windows/`；详细说明见 `docs/RELEASE.md`。UI 和日志文案使用简体中文，后续可在不改变 Core 协议的前提下增加多语言资源层。

While the desktop bundle is missing its icon, validate the implemented Core independently:

```powershell
cargo fmt --check
cargo check -p remote-env-core
cargo test -p remote-env-core
```

## Working agreement

1. Read `CONTEXT.md`, `ARCHITECTURE.md`, and this file before changes.
2. Read server API docs and active server code before protocol changes.
3. Never modify `RemoteEnvServer` in this workstream.
4. Commit each completed logical unit.
5. Update `CONTEXT.md` and `CHANGELOG.md` after every completed phase.
6. Hardware claims need real API/device evidence, not compilation alone.

## Phase 1.75-C runtime integration

- `RuntimeSupervisor` no longer enqueues new events into legacy `upload_queue`; it allocates the global sequence and calls `UploadDispatcher.persist_event`.
- `DispatcherSupervisor` owns target selection and worker lifecycle. Profile removal stops the worker and cancels its existing deliveries; URL/token changes stop, recover, and replace the worker.
- `ServerWorker` has an independent stop-aware WebSocket loop, heartbeat, reconnect backoff, strict target ACK, in-flight recovery, and `Blocked` authentication state.

- `RuntimeSupervisor` now supports an optional platform collector callback. Windows desktop passes `NativeWlanProvider` through `start_with_collector`; scan results enter the same bounded event channel and upload pipeline as other CollectorEvents. It uses temporary SQLite state and two independently bound local WebSocket listeners; tests wait on bounded predicates and stop the Runtime before returning. The Rust ignored smoke skeleton remains available for protocol experiments; the corrected endpoint was verified successfully by the standalone Python client. No credentials are stored in the repository. The full workspace gate currently passes.

## Windows Wi-Fi manual check

Run `cargo run -p remote-env-platform-windows --example windows_wifi_scan` on a Windows host with WLAN service and a supported adapter. The command prints only interface count, BSS count, and duration. It does not print SSIDs, BSSIDs, credentials, or tokens.

The local end-to-end Wi-Fi fixture is `cargo test -p remote-env-core --test phase2a`. It uses a mock snapshot provider and a real local WebSocket fixture. The environment-only real backend smoke test additionally requires `REMOTE_ENV_REAL_TEST=1`, `REMOTE_ENV_TEST_URL`, `REMOTE_ENV_TEST_DEVICE_ID`, `REMOTE_ENV_TEST_TOKEN`, and an explicit `REMOTE_ENV_TEST_SEQUENCE`.

Phase 2 stability verification also runs `cargo test --workspace` and `npm --prefix app/ui run build`. The heartbeat negative-path fixture waits for the production 60-second no-pong threshold. Release verification must use `pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1`, then inspect the nested Portable EXE and NSIS artifact under `Release/Windows/`; a process smoke is separate from real desktop UI clicks.

## Windows Bluetooth validation

Run `cargo run -p remote-env-platform-windows --example windows_bluetooth_scan`. The probe performs one unified BLE + Classic scan and prints only capability, aggregate count, and duration. The local Phase 2-B fixture is `crates/core/tests/phase2b.rs`. It covers one Bluetooth event through Runtime sequence allocation, `upload_deliveries`, local WebSocket ACK completion, dynamic `bluetooth_enabled` enable/disable without a second worker, and two-server target-scoped ACK isolation. Its recovery path closes one server after an unacknowledged upload and verifies the same envelope, device ID, data type, sequence, and payload are resent. No real backend credentials are used.

