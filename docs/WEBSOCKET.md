# WebSocket Lifecycle

## Implemented state machine

```text
Stopped -> Connecting -> Authenticating -> Online
   ^          |              |              |
   |          v              v              v
   +----- Backoff <------ Blocked <----- Disconnecting
```

- Base retry 1 second, doubles to a 30-second cap; retries never exhaust. Jitter is deferred until a long-running reconnect task is composed.
- Authentication sends the collector `auth` frame first and requires `auth_result`.
- Online uses timestamped application `heartbeat` and requires `pong` before three heartbeat intervals elapse.
- Disconnect or timeout returns to Backoff without stopping collectors.
- Non-retryable auth/protocol failures become visible Blocked state.

## Queue contract

Sender owns a bounded durable queue. Items complete only after matching `data_result`; transport failure requeues in-flight work.

| Condition | Planned treatment |
| --- | --- |
| network close/failure | retain and reconnect |
| `rate_limited` | retain and delay |
| `sequence_rejected` | hold; recover valid sequence; never resend unchanged payload |
| invalid token/schema/authorization | block and report |

Queue uses bounded SQLite durable rows. `sequence_rejected` blocks the affected row and returns a dedicated error; because the server exposes no sequence synchronization endpoint, the client does not guess a replacement sequence.

Phase 1.75-B introduces target-scoped delivery records without duplicating the event payload: one global event envelope is represented by one `upload_deliveries` row per selected server. Single mode selects `active_server_id`; Multi mode selects enabled profiles. Each future live server worker must own its own WebSocket state, heartbeat, reconnect backoff, in-flight delivery, and ACK processing. A failure or blocked sequence for one target must not mutate other target rows.

## Phase 1.75-C workers

Each selected target has its own `ServerWorker`: independent WebSocket, heartbeat, reconnect backoff, stop signal, in-flight delivery, ACK matching, and status. `RuntimeSupervisor` stops the `DispatcherSupervisor`, which awaits all worker joins. A worker in `Blocked` state is not retried; network failures remain in capped infinite reconnect. Authentication remains `auth -> auth_result -> device_list -> Ready`.

Local Phase 1.75-C integration tests exercise two independent listeners, same-envelope delivery, target-local ACK/reconnect recovery, profile replacement/removal, rate-limit classification, heartbeat/pong observation, missing-pong reconnect, and permanent auth failure.
