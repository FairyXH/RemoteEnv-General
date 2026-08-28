import React from "react";
import { createRoot } from "react-dom/client";
import "./styles.css";

type CollectorName = "Wi-Fi" | "BLE" | "Classic Bluetooth";

const collectors: Array<{ name: CollectorName; state: string; count: string }> = [
  { name: "Wi-Fi", state: "Not implemented", count: "-" },
  { name: "BLE", state: "Not implemented", count: "-" },
  { name: "Classic Bluetooth", state: "Not implemented", count: "-" },
];

function App() {
  return (
    <main className="shell">
      <header>
        <div>
          <p className="eyebrow">ENVIRONMENT COLLECTOR</p>
          <h1>RemoteEnvCollector</h1>
        </div>
        <span className="state"><i /> Runtime offline</span>
      </header>

      <section className="connection" aria-label="WebSocket status">
        <span className="label">WebSocket</span>
        <strong>Disconnected</strong>
        <span className="muted">Runtime status is not connected to the Tauri shell yet.</span>
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
        <div><span>Upload queue</span><strong>Runtime unavailable</strong></div>
        <div><span>Accepted uploads</span><strong>Runtime unavailable</strong></div>
        <div><span>Last event</span><strong>None</strong></div>
      </section>

      <footer>
        <button type="button" disabled title="Available after the collector runtime is implemented">Start collection</button>
        <span>Phase 1 infrastructure</span>
      </footer>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
