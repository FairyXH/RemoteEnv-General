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
- Multi-target delivery storage and live dispatcher wiring are implemented in the Phase 1.75-C runtime path.
- Target selection is immutable after an event is persisted: changing mode or disabling a profile affects future events only. Removing a server should cancel its pending deliveries rather than leave them indefinitely pending.
- Runtime event persistence now creates one `upload_deliveries` row per immutable target set, reusing the same global sequence for every target.
- The Bluetooth `data.devices` payload follows VirEnvTester: BLE records include parsed fields plus `rawHex`, `rawLength`, and `raw` (Base64 of the complete raw AD/scan-response byte stream). Classic records include only fields available from native inquiry and do not fabricate RAW data.
- Windows Wi-Fi/Bluetooth serializers now use the server field contract directly: Wi-Fi emits `rssi` as a JSON number, `frequency_mhz` as a JSON number, canonical `band` values (`2_4ghz`/`5ghz`/`6ghz`/`unknown`), and `security` as `string[]`; unsupported native fields are not emitted as invented nested objects. Bluetooth emits canonical snake_case fields, `technology` values accepted by the server, full UUIDs, and Base64 for manufacturer/service/raw bytes.
- When both Wi-Fi and Bluetooth collection are enabled, the production runtime sends exactly one combined envelope per compatible snapshot pair using `data_type: "bluetooth"`: `data: {"captured_at_ms": ..., "technology":"bluetooth", "devices": [...], "wifi": <WiFiSnapshot>, "bluetooth": <BluetoothSnapshot>}`. BLE and Classic observations are unified in `devices` and distinguished per device by `mode` (`ble`, `classic`, or `dual`). Both source snapshots must be from events no more than 15 seconds apart; otherwise the older side is discarded and the runtime waits for a fresh pair. One sequence, one delivery, and one ACK represent the combined packet.
- Event completion is true only when all target deliveries are `completed` or `cancelled`; `blocked` remains incomplete and visible.

## Documentation difference

Server docs call the success response a generic data result. Runtime code establishes the exact `data_result` shape above. Client implementation follows runtime behavior.

## Phase 1.75-C test evidence

Real backend validation passed via the standalone Python client against the corrected endpoint. The client completed TLS WebSocket connection, collector authentication, successful `auth_result`, `device_list` reception, timestamped `heartbeat`/`pong`, one marked `environment_data` upload, and a matching successful `data_result` ACK. Credentials were provided only through process environment variables and were removed after testing.

Phase 2-B local fixtures reuse this exact `environment_data`/`data_result` contract for Bluetooth and do not contact the real backend.

## Phase 2 runtime evidence

The local Phase 2-A/2-B/1.75-C fixtures now assert Unix-millisecond sequence values from the actual envelope, immutable payload/sequence during target-local recovery, explicit collection activation before scanning/upload, and independent A/B ACK completion. Runtime does not synthesize a heartbeat success at authentication time; only JSON `pong` or a WebSocket Pong control frame updates heartbeat freshness. The user-provided real endpoint was verified separately and the Core ignored smoke passed with auth, device_list, environment_data, and matching data_result ACK.

The Core smoke uses a fresh epoch-millisecond sequence and completed successfully against the user-provided endpoint. The one-shot smoke API returns after the matching ACK; the production `ServerWorker` remains long-lived.
