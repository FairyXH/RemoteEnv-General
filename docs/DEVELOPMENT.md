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

Integration verification is in `crates/core/tests/phase175c.rs`. It uses temporary SQLite state and two independently bound local WebSocket listeners; tests wait on bounded predicates and stop the Runtime before returning. The Rust ignored smoke skeleton remains available for protocol experiments; the corrected endpoint was verified successfully by the standalone Python client. No credentials are stored in the repository. The full workspace gate currently passes.

## Windows Wi-Fi manual check

Run `cargo run -p remote-env-platform-windows --example windows_wifi_scan` on a Windows host with WLAN service and a supported adapter. The command prints only interface count, BSS count, and duration. It does not print SSIDs, BSSIDs, credentials, or tokens.

