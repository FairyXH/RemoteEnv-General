# Current Context

## Read this first

Read this file, `ARCHITECTURE.md`, and `DEVELOPMENT.md` before changes. For transport work also read `PROTOCOL.md` and `WEBSOCKET.md`.

## Current state

Phase 2-A is implemented and locally validated. Phase 2-B Windows BLE + Classic Bluetooth is complete locally: unified scanning, Runtime, delivery, multi-server recovery, raw AD preservation, UI, tests, and real hardware probe all pass. Real backend upload remains intentionally untested.

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
- `cargo test --workspace`: PASS; Core unit 2, Core phase1 14 passed/1 ignored, Phase 1.75-C 8, Phase 2-A 2, Windows platform 4.
- `app/ui`: `npm run build` PASS.
- `cargo run -p remote-env-platform-windows --example windows_wifi_scan`: PASS, 1 interface, 10 networks, 1501 ms.
- `netsh wlan show interfaces`: Hardware On, Software On.
- `netsh wlan show networks mode=bssid`: 6 visible SSID groups, multiple BSS entries.
- Security scan: no `rev1_` matches; `CollectTestor` appears only in the explicit environment-based smoke-test code/docs; no credential value is stored.

## Next step

Run the environment-only real backend Wi-Fi smoke test with user-provided `REMOTE_ENV_TEST_URL`, `REMOTE_ENV_TEST_DEVICE_ID`, `REMOTE_ENV_TEST_TOKEN`, and `REMOTE_ENV_TEST_SEQUENCE`, then clean all variables. If it succeeds, update this file and the changelog with the actual ACK evidence and mark Phase 2-A Complete. Do not start Phase 2-C in this session.

## Existing phase history

Phase 1.75-C is Complete: RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker(s), target delivery persistence, independent WebSocket workers, recovery, heartbeat, ACK isolation, and real backend protocol verification are complete. The old Phase 1.5/1.75-B notes remain below in Git history/docs for continuity.

