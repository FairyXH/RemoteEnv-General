# RemoteEnvServer Protocol Notes

Source: `D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\docs\api.md`; verified against `remote_env_server/bus.py` and `models.py` on 2026-08-27.

## Connection

- Endpoint: `ws://host:port/ws` or a prefixed deployment endpoint, e.g. `wss://host/envser/ws`.
- Frames: UTF-8 JSON text frames.
- First frame: `auth`, `role: "collector"`, device Token, Token-bound `device_id`, and required device metadata.
- Success: `auth_result`, then `device_list`.
- Failure: `{type:"error", code, message, retryable}`.

## Envelope

```json
{"type":"environment_data","version":1,"device_id":"collector-001","data_type":"wifi","timestamp":1760000000000,"sequence":42,"data":{"networks":[]}}
```

Time is Unix milliseconds. `data_type` must match `^[a-z][a-z0-9_.-]{0,63}$`.

## Verified implementation details

- Accepted upload ACK is `{type:"data_result",success:true,device_id,data_type,sequence}`.
- `sequence` must be strictly increasing per `(device_id, data_type)`, including after restart and reconnect. Client state must therefore be durable.
- The server can return `error` instead of `data_result`; sender code must classify this immediately rather than wait for ACK timeout.
- `sequence_rejected` and `rate_limited` are currently retryable, but a rejected sequence cannot be resent unchanged.
- Current server limit: 60 uploads per device per 60 seconds.

- The client configuration now models `ServerProfile` and `ServerMode`; the same global `(device_id, data_type)` sequence is used for every target.
- Multi-target delivery storage is present as schema groundwork; dispatcher wiring is still partial.
- Target selection is immutable after an event is persisted: changing mode or disabling a profile affects future events only. Removing a server should cancel its pending deliveries rather than leave them indefinitely pending.
- Per-target delivery states are `pending`, `in_flight`, `blocked`, and `cancelled`; an ACK removes only the matching target row.

## Documentation difference

Server docs call the success response a generic data result. Runtime code establishes the exact `data_result` shape above. Client implementation follows runtime behavior.
