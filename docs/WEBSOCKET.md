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

`RuntimeSupervisor` owns a dedicated Tokio runtime thread and a bounded `mpsc` event channel. It keeps calling the WebSocket manager after failures, recovers in-flight rows, publishes status snapshots through a watch channel, and stops through a oneshot signal. The current sender/receiver loop is implemented within the transport session; splitting it into independently managed tasks is deferred until the protocol needs concurrent application messages.
