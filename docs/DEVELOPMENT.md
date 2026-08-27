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

## Working agreement

1. Read `CONTEXT.md`, `ARCHITECTURE.md`, and this file before changes.
2. Read server API docs and active server code before protocol changes.
3. Never modify `RemoteEnvServer` in this workstream.
4. Commit each completed logical unit.
5. Update `CONTEXT.md` and `CHANGELOG.md` after every completed phase.
6. Hardware claims need real API/device evidence, not compilation alone.
