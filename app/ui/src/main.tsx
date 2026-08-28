import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type CollectorName = "Wi-Fi" | "Bluetooth";

const collectors: Array<{ name: CollectorName; state: string; count: string }> = [
  { name: "Wi-Fi", state: "Not implemented", count: "-" },
  { name: "Bluetooth", state: "Disabled", count: "-" },
];

type RuntimeStatus = {
  connection: string;
  wifi: string;
  wifi_runtime: { enabled: boolean; state: string; network_count: number | null; last_scan_ms: number | null; last_successful_scan_ms: number | null; duration_ms: number | null; last_error: string | null; total_scans: number; successful_scans: number; failed_scans: number };
  bluetooth: string;
  bluetooth_runtime: { enabled: boolean; state: string; device_count: number | null; ble_device_count: number; classic_device_count: number; last_scan_ms: number | null; last_successful_scan_ms: number | null; duration_ms: number | null; last_error: string | null; total_scans: number; successful_scans: number; failed_scans: number };
  pending: number;
  in_flight: number;
  blocked: number;
  uploaded: number;
  failed: number;
  servers: Array<{ profile_id: string; connection: string; pending: number; in_flight: number; blocked: number }>;
};

const initialStatus: RuntimeStatus = {
  connection: "Disconnected",
  wifi: "Disabled",
  wifi_runtime: { enabled: false, state: "Disabled", network_count: null, last_scan_ms: null, last_successful_scan_ms: null, duration_ms: null, last_error: null, total_scans: 0, successful_scans: 0, failed_scans: 0 },
  bluetooth: "Disabled",
  bluetooth_runtime: { enabled: false, state: "Disabled", device_count: null, ble_device_count: 0, classic_device_count: 0, last_scan_ms: null, last_successful_scan_ms: null, duration_ms: null, last_error: null, total_scans: 0, successful_scans: 0, failed_scans: 0 },
  pending: 0,
  in_flight: 0,
  blocked: 0,
  uploaded: 0,
  failed: 0,
  servers: [],
};

function App() {
  const [status, setStatus] = React.useState(initialStatus);
  const [runtimeAvailable, setRuntimeAvailable] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let active = true;
    const refresh = () => invoke<RuntimeStatus>("get_runtime_status")
      .then((next) => { if (active) { setStatus(next); setRuntimeAvailable(true); } })
      .catch(() => { if (active) setRuntimeAvailable(false); });
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const start = () => invoke("start_runtime")
    .then(() => setError(null))
    .catch((reason) => setError(String(reason)));

  return (
    <main className="shell">
      <header>
        <div>
          <p className="eyebrow">ENVIRONMENT COLLECTOR</p>
          <h1>RemoteEnvCollector</h1>
        </div>
        <span className="state"><i /> {runtimeAvailable ? status.connection : "Runtime offline"}</span>
      </header>

      <section className="connection" aria-label="WebSocket status">
        <span className="label">WebSocket</span>
        <strong>{runtimeAvailable ? status.connection : "Disconnected"}</strong>
        <span className="muted">{error ?? (runtimeAvailable ? "Live Core Runtime status" : "Runtime status is not connected to the Tauri shell yet.")}</span>
      </section>

      <section className="group" aria-label="Servers">
        <div className="section-heading"><h2>Servers</h2><span>{status.servers.length} active</span></div>
        {status.servers.map((server) => (
          <div className="row" key={server.profile_id}>
            <div><strong>{server.profile_id}</strong><span>{server.connection}</span></div>
            <b>{server.pending + server.in_flight} queued</b>
          </div>
        ))}
      </section>

      <section className="group" aria-label="Collectors">
        <div className="section-heading"><h2>Collectors</h2><span>{status.wifi_runtime.enabled || status.bluetooth_runtime.enabled ? "Enabled" : "Disabled"}</span></div>
        {collectors.map((collector) => (
          <div className="row" key={collector.name}>
            <div><strong>{collector.name}</strong><span>{collector.name === "Wi-Fi" ? status.wifi : status.bluetooth}</span></div>
            <b>{collector.name === "Wi-Fi" ? (status.wifi_runtime.network_count === null ? "No scan" : `${status.wifi_runtime.network_count} APs`) : (status.bluetooth_runtime.device_count === null ? "No scan" : `${status.bluetooth_runtime.device_count} devices`)}</b>
          </div>
        ))}
      </section>

      {status.wifi_runtime.last_error && <p className="muted">Wi-Fi error: {status.wifi_runtime.last_error}</p>}
      {status.bluetooth_runtime.last_error && <p className="muted">Bluetooth error: {status.bluetooth_runtime.last_error}</p>}

      <section className="metrics" aria-label="Runtime statistics">
        <div><span>Upload queue</span><strong>{runtimeAvailable ? `${status.pending} pending / ${status.in_flight} sending` : "Runtime unavailable"}</strong></div>
        <div><span>Accepted uploads</span><strong>{runtimeAvailable ? status.uploaded : "Runtime unavailable"}</strong></div>
        <div><span>Wi-Fi scan</span><strong>{status.wifi_runtime.duration_ms === null ? "No scan yet" : `${status.wifi_runtime.duration_ms} ms / ${status.wifi_runtime.total_scans} scans`}</strong></div>
        <div><span>Bluetooth scan</span><strong>{status.bluetooth_runtime.duration_ms === null ? "No scan yet" : `${status.bluetooth_runtime.duration_ms} ms / ${status.bluetooth_runtime.total_scans} scans`}</strong></div>
      </section>

      <footer>
        <button type="button" onClick={start}>Start runtime</button>
        <span>Phase 1 infrastructure</span>
      </footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
