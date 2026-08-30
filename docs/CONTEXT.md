# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current round: RemoteEnvServer API normalization and Windows Release icon

Status: Implemented, tested, and sequence-fix Release-verified.

后续修正：用户反馈真实服务端返回 `sequence_rejected / sequence is not newer`。已统一所有生产 sequence 入口：`StateStore::next_sequence` 委托 Unix 毫秒分配，`EnvironmentEnvelope::with_timestamp` 也会将传入 sequence 提升到当前 Unix 毫秒下限，`sequence_rejected` 恢复使用 `max(now_ms, rejected + 1)`，旧持久状态不会回退为 1/2 等计数值。相关测试已改为验证 Unix 毫秒量级和严格递增。

This round audited the active Windows/Core payload boundary against `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\docs\api.md`. Wi-Fi now serializes `rssi`, numeric `frequency_mhz`, canonical band values, and `security: string[]`; guessed nested security metadata and Windows-only fields are not uploaded. Bluetooth now uses server-compatible snake_case fields, accepted `technology` values, full 128-bit UUID strings, Base64 for binary values, and preserves diagnostic `mode` only as an extension. Sequence allocation keeps the required `max(now_ms, last+1)` monotonic semantics, and envelope timestamps use the collector event timestamp rather than a later arbitrary wall-clock value.

The Tauri icon was rebuilt from `app/ui/res/mipmap-xxxhdpi/logo.png` into a multi-size `app/desktop/src-tauri/icons/icon.ico`; `tauri.conf.json` already points to this icon for Windows bundles.

本轮最终验证：`cargo fmt --all` PASS；`cargo check --workspace` PASS；`cargo test --workspace` PASS（14 phase1 tests + 1 ignored real-backend smoke、8 phase175c、3 phase2a、3 phase2b、Windows 12 tests；缺少环境变量的 real-backend smoke 未执行）；后续补充 `cargo check --workspace` 与 Windows 12 项测试在增加 `address_type`/网络 timestamp 后仍 PASS；`app/ui` 的 `npm run build` PASS；`scripts/build-release.ps1` 正式 Windows Release PASS。最终产物：`Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe`（12,438,528 bytes）和 `Release/Windows/RemoteEnvCollector-Setup.exe`（3,101,560 bytes）。复制后的 EXE 启动并保持存活 5 秒后停止，属于进程冒烟而非完整 UI 点击验收。服务器端 Pydantic 运行时验证未执行，因为当前 Server 项目 Python 环境缺少 `fastapi` 依赖；协议字段依据已读取的 `api.md` 与 `models.py` 审查，并由本地 Rust fixtures 覆盖。

本轮工作区仍包含此前已存在的 Tauri 生命周期、Core 测试和 Release 二进制变更；未回滚。sequence 修正源代码已通过本地检查/测试，正式 Release 已重建并完成 EXE 5 秒进程冒烟；当前仍未完成：真实 Windows Wi-Fi/Bluetooth 后端上传复测、安装包安装验收、WebView2 UI 自动点击/截图 E2E。下一步应先完成真实后端复测，再在可用的交互式 Windows UI 自动化环境中完成其他验收；在此之前 Phase 2-C 继续保持 Partial。

2026-08-30 sequence follow-up: 用户实际日志显示服务端拒绝旧 sequence。已修正 `StateStore::next_sequence`、`Runtime`、`EnvironmentEnvelope::with_timestamp` 和 `ServerWorker` recovery，所有新建/恢复 sequence 都至少为当前 Unix epoch milliseconds，并保持持久状态严格递增。测试 `phase1` 14 passed/1 ignored，`cargo check --workspace` passed。该修正尚需正式 Release 重建和真实后端复测。

Sequence follow-up verification: `cargo check --workspace` PASS；`cargo test --workspace` PASS（phase1 14 passed/1 ignored、phase175c 8 passed、phase2a 3 passed、phase2b 3 passed、Windows 12 passed）；`app/ui` `npm run build` PASS；正式 `scripts/build-release.ps1` PASS。最终 sequence-fix Release：`Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` 12,438,528 bytes，`Release/Windows/RemoteEnvCollector-Setup.exe` 3,101,560 bytes；EXE 启动 5 秒存活后停止。真实后端 sequence 复测尚未执行。

Windows BLE observations now retain the complete AD/scan-response byte stream and serialize the server/VirEnvTester-compatible fields on each `data.devices` record: `rawHex`, `rawLength`, and `raw`, where `raw` is standard ASCII Base64. Classic Bluetooth records keep these fields absent because the native inquiry API does not provide advertisement bytes. The existing parsed fields and `rawAdvertisementSections` remain available; this change does not alter `data_type` (`bluetooth`) or the combined envelope structure.

Verified: `cargo fmt --all` passed; `cargo check --workspace` passed; `cargo test -p remote-env-platform-windows` passed (12 tests), including exact Base64 regression coverage; `cargo test --workspace` passed; `app/ui` `npm run build` passed. Formal Windows Release build passed with `scripts/build-release.ps1`. Final artifacts: `Release/Windows/RemoteEnvCollector/RemoteEnvCollector.exe` (12,461,056 bytes) and `Release/Windows/RemoteEnvCollector-Setup.exe` (3,101,083 bytes). The copied Release EXE launched and remained alive for 5 seconds, then was stopped. This is process smoke evidence, not manual UI click E2E; real hardware Bluetooth scan and real backend upload were not rerun in this round.

Core files changed: `crates/core/src/bluetooth.rs`, `crates/platform-windows/src/bluetooth/ble.rs`, `crates/platform-windows/src/bluetooth/classic.rs`, `crates/platform-windows/src/bluetooth/model.rs`, `crates/platform-windows/src/bluetooth/collector.rs`; protocol/session docs updated. Commit: `635aa84` (`fix: upload Windows Bluetooth raw data as Base64`). The worktree still contains pre-existing unrelated source changes and generated Release binary modifications; they were not staged or reverted. Phase 2-B remains locally complete. This RAW format round is complete for code, tests, UI build, and Release packaging; manual desktop UI click acceptance and fresh real backend/hardware evidence remain outside this round.


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

`9b5e530`, `acff961`, `6bef0ac`, `c26afd6`, `f2e8d2c`, `1cb6f99`, `58d0851`, `9234c10`, `5208bfa`, `92b24aa`, `b2ef4ac`, `d629021`.

## Known issues / next step

- Real packaged UI button E2E remains the blocking acceptance item. Install/enable a working Windows desktop automation path for Tauri WebView2 or run the required manual click matrix in an interactive desktop session.
- The 60-second missing-pong regression intentionally takes about 60 seconds.
- Working tree contains the existing `build-release-windows.bat` modification and generated Release binary state; do not revert unrelated changes.
- Do not report Phase 2 Runtime/UI Stability as Complete until the Release UI is actually operated through Start, continuous scans/uploads, heartbeat display, disconnect/reconnect, and one-click Stop.

2026-08-29 follow-up: AppData SQLite remained valid and the user-provided real backend continued to pass auth/device_list/environment_data/data_result. A live inspection found multiple simultaneous Release EXE processes during the reported UI failure; the final Release now enforces one process using a Windows named Mutex. UI initialization and runtime-option errors now include the exact failing command (`get_runtime_status` versus `get_desktop_config`) and are logged by the Tauri backend. Latest `cargo check --workspace` and `npm run build` pass, and launching the final EXE twice leaves one process. Final packaged UI click E2E remains unverified because WebView2 UIA automation is unavailable.

2026-08-29 ACL/mode follow-up: Tauri capability `app/desktop/src-tauri/capabilities/default.json` now grants `core:event:default`, fixing `Command plugin:event|listen not allowed by ACL`. `set_runtime_options` now normalizes Multi -> Single: it preserves an enabled active profile, otherwise selects the first enabled profile, and if all profiles exist but are disabled it enables the first profile before validation. Only an empty profile set can remain without an active server. `cargo check --workspace`, `cargo test --workspace`, `npm run build`, and the Release build passed after this change.

## User-provided real backend verification

2026-08-29: 使用用户提供的 WSS、`device_id=test` 和 Token 进行真实验证。独立 Python frame probe 完成 `auth_result.success=true`、收到 8 个设备的 `device_list`、发送一条 `environment_data(data_type=wifi)`，并收到匹配的 `data_result.success=true`。随后使用同一组仅进程环境变量和新 Unix 毫秒 sequence 运行 Core ignored smoke，测试通过。此前 `Transport(Utf8)` 根因是旧 `WebSocketManager` 在认证、ACK 和 heartbeat 阶段直接对控制帧调用 `to_text()`；另一个根因是 `run_once()` ACK 后不返回。两者已修复。凭据已从进程环境清除，未写入代码、文档或持久配置。

后续用例 `cargo test --workspace` 通过，`npm run build` 通过，最新 Release 也通过。旧 Core `WebSocketManager` 的真实 smoke 现在在成功 ACK 后正常返回；服务端真实证据为 `auth_result=True`、`device_list=8`、`data_result=True`。

本机 AppData SQLite `integrity_check` 通过，配置 URL/device ID 正确，Token 为 DPAPI 保护内容。故障期间曾同时运行 4 个 Collector EXE，日志显示重复连接请求；已加入 Windows named Mutex `Global\\RemoteEnvCollector.SingleInstance`，阻止多实例争用同一 AppData 状态。

## Existing phase history

## Combined snapshot upload round

The runtime now keeps Wi-Fi and Bluetooth collectors running independently but persists/upload events as one `data_type="bluetooth"` envelope when both collectors are enabled. The envelope contains one `captured_at_ms`, top-level `data.devices`, `data.wifi`, and `data.bluetooth`; BLE and Classic observations are unified in `devices` with per-device `mode`. A pair is emitted only when the two source event timestamps are within a fixed 15-second skew. Sequence and ACK count therefore advance once per combined packet, not once per radio.

RuntimeStatus now carries the latest `wifi_snapshot` and `bluetooth_snapshot`, so the Tauri status event continuously updates card counts and detail dialogs without manual scans. Windows BLE and Classic scans execute concurrently; BLE callback delivery no longer silently drops observations when a bounded channel fills, Classic inquiry uses a longer timeout, and BluetoothCollector retains a deduplicated 120-second rolling observation window.

Verification: `cargo test --workspace` passed with the combined-envelope regression; `npm run build` and the Release build passed. Real hardware probe reported BLE/Classic availability and the current environment-dependent discovery count. Completeness of radio discovery remains bounded by Windows advertisement/inquiry visibility and cannot be guaranteed beyond devices discoverable at scan time.

The combined-upload implementation is in commits `f792175` and `4b0e6d4`: Wi-Fi and Bluetooth continue scanning independently, but when both latest source events are available within a fixed 15-second skew the Runtime creates one `bluetooth` envelope with one `captured_at_ms`, top-level `data.devices`, `data.wifi`, and `data.bluetooth`. `RuntimeStatus` carries both source payloads to the UI, and server upload success/failure counters are aggregated per delivery ACK/result. BLE callbacks use reliable delivery, BLE and Classic scans execute concurrently, WinRT extended advertisements are enabled, and a reusable BluetoothCollector retains a deduplicated 120-second rolling window. The Bluetooth payload now follows `VirEnvTester`/RemoteEnvServer semantics: one `bluetooth` data type, top-level `devices`, and per-device `mode` (`ble`, `classic`, `dual`). A real endpoint probe accepted this format and returned successful `data_result`. The hardware probe after extended advertisements reported BLE=1 and Classic=3 on this host; this is a radio-environment result, not a guarantee that non-discoverable devices can be enumerated.

Phase 1.75-C is Complete: RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker(s), target delivery persistence, independent WebSocket workers, recovery, heartbeat, ACK isolation, and real backend protocol verification are complete. The old Phase 1.5/1.75-B notes remain below in Git history/docs for continuity.

