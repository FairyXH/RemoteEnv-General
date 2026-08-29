# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current state

Phase 2-C desktop lifecycle repair remains Partial. This round found that the persisted user config had `heartbeat_interval_seconds=15` and `scan_interval_seconds=30`, while the Runtime worker previously hardcoded/used inconsistent values. It also found that existing AppData logs contained only Tauri command-entry records, not worker protocol or scan evidence. Source changes now add safer status intent handling, async supervisor join behavior, richer bridge logging, UI listener reconciliation, per-operation busy guards, and protocol control-frame tolerance. These changes compile, but they are not accepted as behaviorally complete until the full integration tests and real desktop clicks pass.

## Completed

- Read RemoteEnvServer API docs and active WS implementation.
- Chosen Rust 2024 workspace plus Tauri 2 and shared React + TypeScript UI.
- Defined isolated Windows, Android, Linux, and macOS crates.
- Documented exact `data_result` ACK and persistent sequence constraints.
- Windows WLAN API is isolated in `crates/platform-windows` and uses generated `windows-sys` WLAN bindings.
- `RuntimeSupervisor::start_with_collector` runs the platform scan through a stop-aware periodic worker, `spawn_blocking`, dynamic `wifi_enabled`/interval configuration, and bounded timeout.
- `crates/core/tests/phase2a.rs` verifies Wi-Fi payload -> Runtime -> upload delivery -> local WebSocket auth/device_list/upload/ACK -> completed, plus same envelope/sequence recovery after disconnect.
- Real Windows probe with Software Radio On: 1 interface, 10 BSS observations, 1501 ms. `netsh` reported 6 SSID groups and multiple BSS entries.

## Not implemented

- Real backend Wi-Fi upload with `CollectTestor` credentials; credentials were not present in the environment and were not searched for or stored.
- Scan-completion notification callback; current provider uses a bounded 1500 ms wait after `WlanScan` before querying BSS cache.
- Full authentication/encryption IE parsing and persisted scan history.
- Full Tauri tray lifecycle and push-based UI status events.
- Android, Linux, and macOS collectors.

## Phase 2-B status

Status: Complete. Windows BLE uses WinRT `BluetoothLEAdvertisementWatcher`; Classic Bluetooth uses native inquiry APIs. Both enter one `BluetoothCollector`, one `CollectorEvent(data_type="bluetooth")`, one `(device_id, "bluetooth")` sequence namespace, and the existing multi-target delivery pipeline. Runtime owns one Bluetooth worker with `bluetooth_enabled`, shared scan interval, dynamic configuration, source-failure isolation, and truthful status. Unknown BLE AD sections are retained as `source`, `ad_type`, and uppercase `data_hex`.

## Phase 2-A status

Status: Partial. All local code and test gates pass, and real Windows hardware scan passes. The only requested acceptance evidence still missing is real backend Wi-Fi upload and ACK using user-provided environment credentials. Do not mark Complete until that test is executed successfully.

## Verification

- `cargo fmt --check`: PASS
- `cargo check --workspace`: PASS
- `cargo check -p remote-env-core` and `cargo check -p remote-env-desktop`: PASS after the latest source changes.
- `app/ui`: `npm run build`: PASS; latest bundle includes per-operation guards and listener reconciliation.
- Playwright `1.62.0`, Chromium, pywinauto `0.6.9`: installed.
- Native desktop process/UIA discovery: process starts and window `远程环境采集器` is visible; pywinauto enumeration hung on the Tauri/WebView2 tree, and CDP port probing was unavailable. Actual button clicks and DOM assertions: NOT COMPLETED.
- `cargo test -p remote-env-core --test phase175c`: STILL FAILS (3/8 previously; latest run also exposed stale stop assertion and remaining fixture timeouts). Do not mark behavior complete.
- `cargo fmt --all -- --check`: FAIL because pre-existing formatting differences remain in `crates/core/src/state.rs` and `crates/core/tests/phase2b.rs`.
- Rebuilt from cleaned Rust target: PASS; `cargo clean` followed by the Tauri Release pipeline completed from source.
- Release artifacts: `Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` (12,103,680 bytes), `Release/Windows/RemoteEnvCollector-Setup.exe` (3,059,089 bytes), both version `0.2.0`.
- Packaged nested EXE process smoke: PASS; remained alive for 5 seconds and was then stopped. The root-level `Release/Windows/RemoteEnvCollector.exe` path is not produced by the current script.
- New source Release rebuild after latest changes: PASS; `release-build-new.log` confirms fresh React build, Tauri optimized compilation, and NSIS packaging.
- New artifacts: `Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` (12,094,464 bytes) and `Release/Windows/RemoteEnvCollector-Setup.exe` (3,059,956 bytes), both version `0.2.0`.
- Latest source changes (listener installation no longer reports a false failure, pending connection state is retained until Ready, and per-operation state is preserved) pass `npm run build` and `cargo check -p remote-env-desktop`.
- `cargo run -p remote-env-platform-windows --example windows_wifi_scan`: PASS, 1 interface, 10 networks, 1501 ms.
- `netsh wlan show interfaces`: Hardware On, Software On.
- `netsh wlan show networks mode=bssid`: 6 visible SSID groups, multiple BSS entries.
- Root build entrypoint: `build-release-windows.bat` uses `%~dp0`/relative paths, runs `cargo clean`, invokes `scripts/build-release.ps1`, writes the root log, validates EXE/installer existence and size, then pauses for double-click visibility. The BAT was executed via `cmd`, but its console output is not reliable in the non-interactive tool host; the direct PowerShell release script run succeeded and produced the artifacts below.
- Latest direct release verification log: `release-build-final.log`; React build, Tauri optimized build, and NSIS bundle all PASS.
- Latest artifacts: `Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` (12,094,464 bytes) and `Release/Windows/RemoteEnvCollector-Setup.exe` (3,059,986 bytes), version `0.2.0`; packaged EXE remained alive and responsive for 5 seconds.
## Phase 2-C status

Status: Partial. Windows Release pipeline, unified version `0.2.0`, Chinese desktop UI, server profile CRUD, token masking, runtime status push, and close-to-tray lifecycle are implemented. Portable artifact generation is available through `scripts/build-release.ps1`. Clean-machine execution, installer execution, and final release smoke evidence remain pending.

## Next step

First isolate the remaining Phase 1.75-C timeout (inspect worker status and fixture frame order rather than weakening tests). Then add regression coverage for Connect-only transitional status, five-second heartbeat freshness reset, immediate scan on Start, and Stop latency with a blocking scanner. Only after the workspace suite is green should the Tauri release build and real WebDriver/native desktop E2E be run. Current workspace has the two lifecycle commits `49865ec` and `98c63b3` plus the uncommitted context update; do not mark Phase 2-C complete.

## Existing phase history

Phase 1.75-C is Complete: RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker(s), target delivery persistence, independent WebSocket workers, recovery, heartbeat, ACK isolation, and real backend protocol verification are complete. The old Phase 1.5/1.75-B notes remain below in Git history/docs for continuity.

