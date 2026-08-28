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

Production path: `Platform collector -> normalized CollectorEvent -> RuntimeSupervisor -> UploadDispatcher -> upload_deliveries -> DispatcherSupervisor -> ServerWorker(s) -> independent WebSocket -> RemoteEnvServer`.

Collectors must not call WebSocket APIs directly. UI reads application status snapshots and sends commands; it does not call OS APIs.

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

## Phase 2-A Runtime integration

`RuntimeSupervisor::start_with_collector` starts an optional platform scan callback on a dedicated worker runtime. Scans run through `spawn_blocking`, on the configured `scan_interval_seconds`, and stop checks prevent new work after shutdown. A bounded 120-second timeout converts a hung scan into a Wi-Fi Error state; the Runtime continues and later cycles can retry. Runtime config updates apply `wifi_enabled` and the interval without restarting the process.

## Phase 2-A verification

`crates/core/tests/phase2a.rs` drives a complete local Wi-Fi envelope through Runtime sequence allocation, target delivery persistence, an actual local WebSocket session, exact `data_result`, and SQLite completion. Its recovery case forces an unacknowledged connection close and confirms the same `(device_id, data_type, sequence, data)` is observed again before ACK completion. The environment-only real backend Wi-Fi run remains pending because no Token was available in the current process environment.
