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

Phase 1.75 uses a dedicated Tokio runtime thread and bounded event channel. `RuntimeSupervisor` owns reconnect and persistence recovery; Tauri owns lifecycle and commands. Real backend smoke tests are ignored by default and read `REMOTE_ENV_REAL_TEST`, `REMOTE_ENV_TEST_URL`, `REMOTE_ENV_TEST_DEVICE_ID`, and `REMOTE_ENV_TEST_TOKEN` only from the process environment. Never store these values in the repository.
