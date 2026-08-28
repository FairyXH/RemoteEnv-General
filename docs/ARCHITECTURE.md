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

Production path: `Platform collector -> normalized CollectorEvent -> bounded upload queue -> WebSocket sender -> RemoteEnvServer`.

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

`ClientConfig` contains `ServerProfile` records (`id`, `name`, `url`, `token`, `enabled`) and `ServerMode::{Single, Multi}`. Single mode selects `active_server_id`; Multi mode selects all enabled profiles. Tokens are persisted with the local configuration store but are never included in status snapshots or logs. The delivery table has been added as schema groundwork; the active runtime still uses the legacy single queue until target dispatch is completed.

## Phase 1.75-B dispatcher

`UploadDispatcher` resolves the target set once per event and stores one `upload_deliveries` row per selected server. The global `(device_id, data_type, sequence)` is reused for all targets. ACK, recovery, block, cancel, and status operations are scoped by `target_id`; the dispatcher does not yet own live per-profile WebSocket workers.
