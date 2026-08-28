# Windows Wi-Fi Research

## Scope

Phase 1.75 does not implement Windows scanning. This note records the API choice for Phase 2.

## Recommended API

Use the native WLAN Client API through a small Windows-only adapter in `remote-env-platform-windows`:

- `WlanOpenHandle`
- `WlanEnumInterfaces`
- `WlanScan`
- `WlanGetAvailableNetworkList`
- `WlanGetNetworkBssList`
- `WlanCloseHandle`

`WlanGetNetworkBssList` is preferred for per-BSSID observations and RSSI. `WlanGetAvailableNetworkList` is useful for profile/security summaries but may collapse multiple BSS entries.

## Data mapping

- SSID: `DOT11_SSID` bytes plus length; preserve hidden/zero-length SSID as hidden.
- BSSID: `DOT11_MAC_ADDRESS`.
- RSSI: BSS `lRssi`; signal percentage from available-network entries.
- Channel/frequency/band: derive from center frequency where the driver provides it; keep raw frequency and derived band.
- Security/authentication/encryption: available-network security attributes; do not infer unsupported fields.
- Multiple BSSID: retain one record per BSS.
- Current connection: identify the interface and connection state separately from scan results.

## Constraints

The WLAN API is available to normal desktop applications, but driver behavior varies. `WlanScan` is asynchronous and may be rate-limited by the adapter/driver. It can temporarily contend with roaming or power management; do not scan at every upload tick. Use the configured scan interval, serialize scans per interface, and report stale/partial results rather than blocking the Runtime.

Windows 10/11 compatibility depends primarily on WLAN driver support. 6 GHz support is driver/OS dependent and must be detected from returned frequencies, not assumed. No elevated privilege should be required for ordinary enumeration, but enterprise policy and driver restrictions can deny results.

## Phase 2 implementation plan

1. Add `windows-sys` WLAN bindings only to the Windows platform crate.
2. Build a synchronous minimal scan adapter behind the existing collector boundary, with explicit timeout and resource cleanup.
3. Normalize results into a Core-neutral JSON `CollectorEvent` with scanner metadata and a `source` field.
4. Add Windows fixture/unit tests for byte decoding, hidden SSID, BSSID, frequency-to-band mapping, and security mapping.
5. Validate on Windows 10/11 with multiple adapters and verify that scans do not block queue/WebSocket tasks.
