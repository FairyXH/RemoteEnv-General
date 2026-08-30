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
- Windows Wi-Fi/Bluetooth serializers now use the server field contract directly: Wi-Fi emits `rssi`/`signal_dbm` as JSON numbers, `frequency_mhz` as a JSON number, canonical `band` values (`2_4ghz`/`5ghz`/`6ghz`/`unknown`), and `security` as `string[]`; the top-level `WiFiData` includes `interface`, `is_connected`, `gateway`, `dns_servers`, and `ip_address`, with connection details populated from the live adapter when `is_connected=true` (the server rejects connected payloads without all three). Unsupported native fields are not emitted as invented nested objects. Bluetooth emits canonical snake_case fields, `technology` values accepted by the server, full UUIDs, and Base64 for manufacturer/service/raw bytes.
- Wi-Fi network records include their own positive Unix-millisecond `timestamp`; Bluetooth records include `address_type` (`public`/`random` from WinRT BLE address classification, `unknown` only when the API cannot classify). Bluetooth transport `mode` remains a deliberately documented extension for the UI, while the protocol-facing technology value is canonical. BLE `service_data` is parsed from AD types 0x16/0x20/0x21 into UUID-keyed Base64 values.
- Compatibility migration rewrites legacy Bluetooth payloads with `technology: "bluetooth"` to `technology: "unknown"` before resend. New uploads must never use `bluetooth` as a technology enum; the server accepts only `ble`, `bluetooth_classic`, or `unknown`.
- Wi-Fi and Bluetooth are uploaded as independent envelopes: `data_type="wifi"` with `WiFiData` and `data_type="bluetooth"` with `BluetoothData`. The server selects the schema by `data_type`, so merging Wi-Fi into a Bluetooth envelope would prevent the server from storing/displaying Wi-Fi as a `wifi` type. Per-device UI extensions (`mode`, per-device `technology`, `rawAdvertisementSections`) are stripped at the upload boundary; only server-standard fields are transmitted, while local status snapshots keep the diagnostic fields for the UI.
- Event completion is true only when all target deliveries are `completed` or `cancelled`; `blocked` remains incomplete and visible.

## Documentation difference

Server docs call the success response a generic data result. Runtime code establishes the exact `data_result` shape above. Client implementation follows runtime behavior.

## Phase 1.75-C test evidence

Real backend validation passed via the standalone Python client against the corrected endpoint. The client completed TLS WebSocket connection, collector authentication, successful `auth_result`, `device_list` reception, timestamped `heartbeat`/`pong`, one marked `environment_data` upload, and a matching successful `data_result` ACK. Credentials were provided only through process environment variables and were removed after testing.

Phase 2-B local fixtures reuse this exact `environment_data`/`data_result` contract for Bluetooth and do not contact the real backend.

## Phase 2 runtime evidence

- The local Phase 2-A/2-B/1.75-C fixtures now assert Unix-millisecond sequence values from the actual envelope, immutable payload/sequence during target-local recovery, explicit collection activation before scanning/upload, and independent A/B ACK completion. Runtime does not synthesize a heartbeat success at authentication time; only JSON `pong` or a WebSocket Pong control frame updates heartbeat freshness. The user-provided real endpoint was verified separately and the Core ignored smoke passed with auth, device_list, environment_data, and matching data_result ACK.
- Every client-generated `sequence`, including legacy `next_sequence`, queue-created envelopes, and `sequence_rejected` recovery baselines, is normalized to a current Unix epoch millisecond value and remains strictly increasing over the persisted per-device/type state. A stale persisted sequence is never repaired with a small counter such as `1`, `2`, or `sequence+1` alone.

The Core smoke uses a fresh epoch-millisecond sequence and completed successfully against the user-provided endpoint. The one-shot smoke API returns after the matching ACK; the production `ServerWorker` remains long-lived.
