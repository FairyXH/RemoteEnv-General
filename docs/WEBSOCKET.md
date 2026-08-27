# WebSocket Lifecycle

## Planned state machine

```text
Stopped -> Connecting -> Authenticating -> Online
   ^          |              |              |
   |          v              v              v
   +----- Backoff <------ Blocked <----- Disconnecting
```

- Base retry 1 second, double to 60-second cap with jitter; retries never exhaust.
- Authentication sends the collector `auth` frame first and requires `auth_result`.
- Online uses application `ping` and requires `pong` before timeout.
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

Queue persistence and overflow policy are Phase 1 design decisions.
