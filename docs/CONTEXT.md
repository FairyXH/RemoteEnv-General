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

## Phase 2 Runtime/UI stability round

Status: **Partial**. This round fixed command delivery and lifecycle issues: Runtime commands now use an unbounded FIFO channel, Runtime and collector workers are joined on stop, desktop Stop removes and synchronously stops the Runtime, and cached events are flushed immediately when collection is enabled. Persistence/dispatcher errors are no longer silently discarded. Server workers use the configured heartbeat cadence, send JSON `{"type":"ping"}` independently, require real pong frames for heartbeat freshness, and force reconnect after 60 seconds without pong. Existing persisted heartbeat intervals are migrated to 5 seconds after decrypting profile tokens. The UI now renders heartbeat health as green <=5s, yellow >5s, and red >30s/>60s.

Root causes found: Phase 2 fixtures did not issue the explicit collection activation command; old fixtures asserted sequence values `1/2` although production now allocates Unix-millisecond sequences; one recovery fixture notified server A instead of B; Runtime used bounded `try_send`; stop discarded join handles; and the UI/worker path initially treated authentication completion as heartbeat success. These were corrected and covered by local fixtures.

## Verification

- `cargo check --workspace`: PASS.
- `cargo test --workspace`: PASS. Core/platform counts: 2 unit tests, 14 phase1 tests plus 1 explicitly ignored real-backend smoke, 8 phase175c tests, 2 phase2a tests, 3 phase2b tests, and 11 Windows platform tests. The ignored real-backend test was not executed because credentials were not present.
- `npm run build` from `app/ui`: PASS; latest production bundle built successfully.
- `cargo fmt --all -- --check`: FAIL due pre-existing formatting differences in `app/desktop/src-tauri/src/lib.rs` and `crates/core/src/state.rs`; no formatting-only sweep was applied.
- Release build: PASS through direct `pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/build-release.ps1`. Final artifacts are `Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` (12,110,848 bytes) and `Release/Windows/RemoteEnvCollector-Setup.exe` (3,061,214 bytes), both version `0.2.0`. The current script intentionally does not produce the legacy root-level EXE.
- Release process smoke: EXE was launched from the final nested Release path and WebView2 child processes were observed. Main-process/window enumeration through pywinauto UIA timed out; actual click/input/read/screenshot E2E was **NOT COMPLETED**. This is not treated as UI acceptance.
- Real backend Wi-Fi/Bluetooth upload, ACK,断线恢复: **NOT EXECUTED**; no credentials were available and none were searched for or stored.

## Commits in this round

`9b5e530`, `acff961`, `6bef0ac`, `c26afd6`, `f2e8d2c`, `1cb6f99`, `58d0851`, `9234c10`, `5208bfa`, `92b24aa`, `b2ef4ac`.

## Known issues / next step

- Real packaged UI button E2E remains the blocking acceptance item. Install/enable a working Windows desktop automation path for Tauri WebView2 or run the required manual click matrix in an interactive desktop session.
- The 60-second missing-pong regression intentionally takes about 60 seconds.
- Working tree contains the existing `build-release-windows.bat` modification and generated Release binary state; do not revert unrelated changes.
- Do not report Phase 2 Runtime/UI Stability as Complete until the Release UI is actually operated through Start, continuous scans/uploads, heartbeat display, disconnect/reconnect, and one-click Stop.

## User-provided real backend verification

2026-08-29: 使用用户提供的 WSS、`device_id=test` 和 Token 进行真实验证。独立 Python frame probe 完成 `auth_result.success=true`、收到 8 个设备的 `device_list`、发送一条 `environment_data(data_type=wifi)`，并收到匹配的 `data_result.success=true`。随后使用同一组仅进程环境变量和新 Unix 毫秒 sequence 运行 Core ignored smoke，测试通过。此前 `Transport(Utf8)` 根因是旧 `WebSocketManager` 在认证、ACK 和 heartbeat 阶段直接对控制帧调用 `to_text()`；另一个根因是 `run_once()` ACK 后不返回。两者已修复。凭据已从进程环境清除，未写入代码、文档或持久配置。

后续用例 `cargo test --workspace` 通过，`npm run build` 通过，最新 Release 也通过。旧 Core `WebSocketManager` 的真实 smoke 现在在成功 ACK 后正常返回；服务端真实证据为 `auth_result=True`、`device_list=8`、`data_result=True`。

本机 AppData SQLite `integrity_check` 通过，配置 URL/device ID 正确，Token 为 DPAPI 保护内容。故障期间曾同时运行 4 个 Collector EXE，日志显示重复连接请求；已加入 Windows named Mutex `Global\\RemoteEnvCollector.SingleInstance`，阻止多实例争用同一 AppData 状态。

## Existing phase history

Phase 1.75-C is Complete: RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker(s), target delivery persistence, independent WebSocket workers, recovery, heartbeat, ACK isolation, and real backend protocol verification are complete. The old Phase 1.5/1.75-B notes remain below in Git history/docs for continuity.

