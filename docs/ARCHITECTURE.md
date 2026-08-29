# Architecture

## Decision

RemoteEnvCollector uses a Rust 2024 workspace and a Tauri 2 application shell with a single React + TypeScript UI. Tauri 2 supports Windows desktop and Android packaging from one UI project while retaining Rust commands and native integration. Tauri is selected over Flutter + `flutter_rust_bridge` because the Windows tray application is an immediate requirement and Android is only a shell in Phase 0; this reduces bridge-specific moving parts while preserving one UI codebase.

## Layers

```text
Shared React UI
        |
Tauri commands / events
        |
Application composition (Tauri shell)
        |
Rust core: models, config, queue, WebSocket runtime contracts
        |
Platform adapter crates
        |
Windows / Android / Linux / macOS APIs
```

Production path: `Wi-Fi/Bluetooth continuous collectors -> latest snapshots -> RuntimeSupervisor pair-at-capture -> one combined CollectorEvent(data_type="bluetooth", data.devices + data.wifi + data.bluetooth) -> UploadDispatcher -> upload_deliveries -> DispatcherSupervisor -> ServerWorker(s) -> independent WebSocket -> RemoteEnvServer`.

Collectors must not call WebSocket APIs directly. UI reads application status snapshots and sends commands; it does not call OS APIs. Normal runtime updates use the Tauri `runtime_status_changed` event; the status command remains the initial/recovery snapshot path.

## Ownership

| Component | Owns | Must not own |
| --- | --- | --- |
| `remote-env-core` | model, protocol, config, runtime contracts | platform APIs or UI state |
| `remote-env-platform-windows` | Windows scanner and capability adapter | protocol serialization or UI rendering |
| `remote-env-platform-android` | future Android/Tauri/Kotlin boundary | copied Windows implementation |
| Linux/macOS crates | capability placeholders and native adapters | Windows conditionals |
| Tauri shell | command/lifecycle/tray wiring | scanner implementation |
| React UI | compact status and operations views | native API access or credential persistence |

## Runtime principles

- Tokio tasks use bounded channels rather than shared mutable collector state.
- Queue capacity and overflow policy must be explicit before collection is enabled.
- Sequence state is durable and strictly monotonic per data type.
- Reconnect is infinite, capped exponential backoff with jitter.
- Tokens never appear in logs or UI status snapshots.

## Multi-server configuration

- `ClientConfig` contains `ServerProfile` records (`id`, `name`, `url`, `token`, `enabled`) and `ServerMode::{Single, Multi}`. Single mode selects `active_server_id`; Multi mode selects all enabled profiles. Tokens are persisted with the local configuration store but are never included in status snapshots or logs. The runtime now persists each event only to `upload_deliveries`; the legacy `upload_queue` is retained solely for compatibility and direct legacy APIs.

## Phase 1.75-B dispatcher

The target-scoped dispatcher groundwork is superseded by the live Phase 1.75-C runtime integration described below.

## Phase 1.75-C runtime integration

`RuntimeSupervisor -> DispatcherSupervisor -> one ServerWorker per selected ServerProfile`. Each worker owns its WebSocket session, heartbeat interval, reconnect backoff, stop signal, in-flight delivery, and status watch channel. Events allocate one durable global sequence and create one target delivery per selected server. ACK and recovery are target-scoped. Removed targets are stopped and their pending/in-flight deliveries are cancelled. Authentication and fatal protocol errors move only that worker to `Blocked`; transport errors reconnect only that target.

Event completion is target-scoped: an event is complete only when every selected delivery is acknowledged or explicitly cancelled. A blocked delivery remains incomplete and visible.

### Phase 1.75-C verification status

`crates/core/tests/phase175c.rs` drives the live `RuntimeSupervisor -> DispatcherSupervisor -> ServerWorker` chain against independent A/B listeners. It verifies dual readiness, identical event envelopes, target-local recovery, Single-to-Multi and Multi-to-Single transitions, profile replacement/removal, rate-limit isolation, heartbeat observation, missing-pong reconnect, ACK isolation, and authentication blocking. The phase is Complete after the local gates and standalone Python real-backend verification passed.

## Phase 2-B Windows Bluetooth Collector

Windows now has one unified Bluetooth collector: `BluetoothCollector -> BLE scanner + Classic Bluetooth scanner -> BluetoothSnapshot`. BLE uses WinRT `BluetoothLEAdvertisementWatcher` with active and extended advertisements, reliable callback collection, `Stop`, `RemoveReceived`, structured fields, and raw `DataSections()` preservation. Classic uses the native Bluetooth inquiry APIs. Runtime owns one Bluetooth worker, uses `bluetooth_enabled` and `scan_interval_seconds`, retains a 120-second deduplicated rolling observation set, and routes the latest snapshot into the combined Wi-Fi/Bluetooth `environment` envelope and independent ServerWorkers. A/B local fixtures cover identical envelopes, ACK isolation, disconnect recovery, and sequence preservation. Unknown raw AD sections use `source`, `ad_type`, and uppercase `data_hex`; scan responses are distinguished when Windows reports them. Real Windows counts are environment-dependent and are not used as the sole acceptance criterion.

`RuntimeSupervisor::start_with_collector` starts optional platform scan callbacks on dedicated worker runtimes. Wi-Fi and Bluetooth scan continuously on the configured interval and stop checks prevent new work after shutdown. The Runtime holds the latest successful snapshot from each source and, when both are enabled, creates one `bluetooth` envelope with server-compatible top-level `devices` plus both Wi-Fi and Bluetooth payloads from a timestamp-compatible pair. A bounded 120-second timeout converts a hung scan into an Error state; the Runtime continues and later cycles can retry. Runtime config updates apply enablement and interval without restarting the process.

Phase 2 runtime stability details: Runtime commands are FIFO and cannot be dropped because a bounded command queue is full. Collection starts disabled, while selected ServerWorkers connect immediately; the explicit collection transition enables scanners and flushes any event already received during startup. Runtime stop joins the supervisor and collector worker threads. ServerWorkers remain independent, use JSON ping every configured 5 seconds, mark freshness only on real pong, and reconnect after 60 seconds without pong. The packaged UI consumes backend status snapshots and computes heartbeat presentation from the last successful pong.

## Phase 2-A verification

`crates/core/tests/phase2a.rs` drives a complete local Wi-Fi envelope through Runtime sequence allocation, target delivery persistence, an actual local WebSocket session, exact `data_result`, and SQLite completion. Its recovery case forces an unacknowledged connection close and confirms the same `(device_id, data_type, sequence, data)` is observed again before ACK completion. The environment-only real backend Wi-Fi run remains pending because no Token was available in the current process environment.
