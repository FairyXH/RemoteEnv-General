import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type CollectorName = "Wi-Fi" | "BLE" | "Classic Bluetooth";

const collectors: Array<{ name: CollectorName; state: string; count: string }> = [
  { name: "Wi-Fi", state: "Not implemented", count: "-" },
  { name: "BLE", state: "Not implemented", count: "-" },
  { name: "Classic Bluetooth", state: "Not implemented", count: "-" },
];

type RuntimeStatus = {
  connection: string;
  pending: number;
  in_flight: number;
  blocked: number;
  uploaded: number;
  failed: number;
  servers: Array<{ profile_id: string; connection: string; pending: number; in_flight: number; blocked: number }>;
};

const initialStatus: RuntimeStatus = {
  connection: "Disconnected",
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
        <div className="section-heading"><h2>Collectors</h2><span>0 active</span></div>
        {collectors.map((collector) => (
          <div className="row" key={collector.name}>
            <div><strong>{collector.name}</strong><span>{collector.state}</span></div>
            <b>{collector.count}</b>
          </div>
        ))}
      </section>

      <section className="metrics" aria-label="Runtime statistics">
        <div><span>Upload queue</span><strong>{runtimeAvailable ? `${status.pending} pending / ${status.in_flight} sending` : "Runtime unavailable"}</strong></div>
        <div><span>Accepted uploads</span><strong>{runtimeAvailable ? status.uploaded : "Runtime unavailable"}</strong></div>
        <div><span>Last event</span><strong>None</strong></div>
      </section>

      <footer>
        <button type="button" onClick={start}>Start runtime</button>
        <span>Phase 1 infrastructure</span>
      </footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
