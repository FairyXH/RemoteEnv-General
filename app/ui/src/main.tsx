import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Server = { id: string; name: string; url: string; device_id: string; enabled: boolean };
type DetailItem = { label: string; value: string };
type Config = { device_id: string; server_mode: "Single" | "Multi"; active_server_id: string | null; server_profiles: Server[]; wifi_enabled: boolean; bluetooth_enabled: boolean; scan_interval_seconds: number; upload_interval_seconds: number };
type RuntimeStatus = { connection: string; wifi: string; bluetooth: string; pending: number; in_flight: number; blocked: number; uploaded: number; servers: Array<{ profile_id: string; connection: string; heartbeat_alive: boolean; last_heartbeat_ms: number | null; pending: number; in_flight: number; blocked: number }>; wifi_runtime: Scan; bluetooth_runtime: Scan };
type Scan = { enabled: boolean; state: string; network_count?: number | null; device_count?: number | null; last_scan_ms: number | null; duration_ms: number | null; successful_scans: number; failed_scans: number; last_error: string | null };

const initialStatus: RuntimeStatus = { connection: "Stopped", wifi: "Stopped", bluetooth: "Stopped", pending: 0, in_flight: 0, blocked: 0, uploaded: 0, servers: [], wifi_runtime: { enabled: false, state: "Stopped", last_scan_ms: null, duration_ms: null, successful_scans: 0, failed_scans: 0, last_error: null }, bluetooth_runtime: { enabled: false, state: "Stopped", last_scan_ms: null, duration_ms: null, successful_scans: 0, failed_scans: 0, last_error: null } };
const emptyConfig: Config = { device_id: "", server_mode: "Single", active_server_id: null, server_profiles: [], wifi_enabled: false, bluetooth_enabled: false, scan_interval_seconds: 1, upload_interval_seconds: 30 };

function ago(value: number | null, now = Date.now()) { if (!value) return "尚未扫描"; return `${Math.max(0, Math.floor((now - value) / 1000))} 秒前`; }
function detailItems(kind: "wifi" | "bluetooth", data: unknown): DetailItem[] {
  const value = data as Record<string, unknown>;
  if (kind === "wifi") {
    const networks = Array.isArray(value.networks) ? value.networks : [];
    return networks.flatMap((item, index) => {
      const network = item as Record<string, unknown>;
      return [
        { label: `网络 ${index + 1}`, value: String(network.ssid ?? "隐藏网络") },
        { label: "BSSID / 信号", value: `${network.bssid ?? "-"} / ${network.signal_strength_dbm ?? "-"} dBm` },
        { label: "频段 / 信道", value: `${network.band ?? "-"} / ${network.channel ?? "-"}` },
        { label: "频率 / 质量", value: `${network.frequency_mhz ?? "-"} MHz / ${network.signal_percent ?? "-"}%` },
        { label: "接口", value: String(network.interface_id ?? "-") },
      ];
    });
  }
  const observations = Array.isArray(value.observations) ? value.observations : [];
  return observations.flatMap((item, index) => {
    const device = item as Record<string, unknown>;
    const raw = Array.isArray(device.raw_advertisement_sections) ? device.raw_advertisement_sections : [];
    const rawText = raw.map(section => {
      const entry = section as Record<string, unknown>;
      return `${entry.source ?? "-"} type=${entry.ad_type ?? "-"} ${entry.data_hex ?? ""}`;
    }).join("; ");
    return [
      { label: `设备 ${index + 1}`, value: String(device.name ?? "未命名设备") },
      { label: "地址 / 类型", value: `${device.address ?? "-"} / ${device.transport ?? "-"}` },
      { label: "信号 / 可连接", value: `${device.rssi ?? "-"} dBm / ${device.connectable == null ? "-" : device.connectable ? "是" : "否"}` },
      { label: "服务 UUID", value: Array.isArray(device.service_uuids) ? device.service_uuids.join(", ") || "-" : "-" },
      { label: "RAW 广告段", value: rawText || "无" },
    ];
  });
}
function stateText(value: string) { const labels: Record<string, string> = { Ready: "已连接", Running: "运行中", Reconnecting: "正在重连", Connecting: "正在连接", Authenticating: "正在认证", Blocked: "已阻止", Stopped: "已停止", Disabled: "已禁用", Starting: "正在启动", Scanning: "正在扫描", Error: "错误" }; return labels[value] ?? value; }

function App() {
  const [status, setStatus] = React.useState(initialStatus);
  const [config, setConfig] = React.useState(emptyConfig);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [editing, setEditing] = React.useState<Server | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [form, setForm] = React.useState({ name: "", url: "", device_id: "", token: "", enabled: true });
  const [showToken, setShowToken] = React.useState(false);
  const [scanData, setScanData] = React.useState<{ wifi: unknown | null; bluetooth: unknown | null }>({ wifi: null, bluetooth: null });
  const [scanSummary, setScanSummary] = React.useState<{ wifi: Partial<Scan>; bluetooth: Partial<Scan> }>({ wifi: {}, bluetooth: {} });
  const [details, setDetails] = React.useState<{ title: string; data: unknown } | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const [scanBusy, setScanBusy] = React.useState({ wifi: false, bluetooth: false });
  const [clock, setClock] = React.useState(Date.now());
  const [logTail, setLogTail] = React.useState("");

  React.useEffect(() => { const timer = window.setInterval(() => setClock(Date.now()), 1000); return () => window.clearInterval(timer); }, []);
  React.useEffect(() => { const timer = window.setInterval(() => { invoke<string>("get_log_tail").then(setLogTail).catch(() => undefined); }, 1000); invoke<string>("get_log_tail").then(setLogTail).catch(() => undefined); return () => window.clearInterval(timer); }, []);

  const reload = React.useCallback(async () => {
    const [nextStatus, nextConfig] = await Promise.all([invoke<RuntimeStatus>("get_runtime_status"), invoke<Config>("get_desktop_config")]);
    setStatus(nextStatus); setConfig(nextConfig);
  }, []);
  React.useEffect(() => { reload().catch(() => setNotice("无法读取应用状态。")); let off: (() => void) | undefined; listen<RuntimeStatus>("runtime_status_changed", (event) => setStatus(event.payload)).then((unlisten) => { off = unlisten; }); return () => off?.(); }, [reload]);
  React.useEffect(() => { if (status.servers.some(server => server.connection === "Blocked")) { window.alert("采集服务启动失败，服务器认证或协议被拒绝，请检查配置和日志。"); } }, [status.servers]);
  const saveOptions = async (next: Partial<Config>) => { try { const result = await invoke<Config>("set_runtime_options", { serverMode: next.server_mode ?? config.server_mode, activeServerId: next.active_server_id ?? config.active_server_id, wifiEnabled: next.wifi_enabled ?? config.wifi_enabled, bluetoothEnabled: next.bluetooth_enabled ?? config.bluetooth_enabled, scanIntervalSeconds: next.scan_interval_seconds ?? config.scan_interval_seconds, uploadIntervalSeconds: next.upload_interval_seconds ?? config.upload_interval_seconds }); setConfig(result); } catch { setNotice("保存采集服务设置失败。") } };
  const openNew = () => { setEditing(null); setDialogOpen(true); setForm({ name: "", url: "", device_id: "", token: "", enabled: true }); setShowToken(false); };
  const edit = (server: Server) => { setEditing(server); setDialogOpen(true); setForm({ name: server.name, url: server.url, device_id: server.device_id, token: "", enabled: server.enabled }); setShowToken(false); };
  const saveServer = async () => { try { const result = await invoke<Config>("save_server_profile", { input: { id: editing?.id, ...form } }); setConfig(result); setDialogOpen(false); setEditing(null); setNotice("服务器配置已保存。"); } catch (error) { setNotice(String(error)); } };
  const testServer = async (id: string) => { if (busy) return; setBusy(`test:${id}`); setNotice("正在测试连接..."); try { const result = await invoke<{ message: string }>("test_server_profile", { id }); setNotice(result.message); } catch (error) { setNotice(String(error)); } finally { setBusy(null); } };
  const removeServer = async (id: string) => { if (!confirm("确定删除此服务器配置吗？")) return; try { setConfig(await invoke<Config>("delete_server_profile", { id })); } catch { setNotice("删除服务器配置失败。") } };
  const toggle = async () => { if (busy) return; setBusy("runtime"); const shouldStart = status.connection === "Stopped" || status.connection === "Reconnecting" || status.connection === "Disconnected"; try { if (shouldStart) { const next = await invoke<RuntimeStatus>("start_runtime"); setStatus(next); setNotice("采集服务已启动，状态将持续更新。"); } else { const next = await invoke<RuntimeStatus>("stop_runtime"); setStatus(next); } } catch (error) { window.alert(`采集服务启动失败：${String(error)}`); } finally { setBusy(null); } };
  const connect = async (id: string) => { if (busy) return; setBusy(`connect:${id}`); setNotice("正在建立持久连接..."); try { const next = await invoke<RuntimeStatus>("connect_server_profile", { id }); setStatus(next); setNotice("服务器连接已启动，状态将持续更新。"); } catch (error) { window.alert(`服务器连接失败：${String(error)}`); } finally { setBusy(null); } };
  const scan = async (kind: "wifi" | "bluetooth") => { if (scanBusy[kind]) return; setScanBusy(current => ({ ...current, [kind]: true })); setNotice(`正在扫描${kind === "wifi" ? " Wi-Fi" : "蓝牙"}...`); try { const event = await invoke<{ data: unknown }>(kind === "wifi" ? "scan_wifi_now" : "scan_bluetooth_now"); setScanData(current => ({ ...current, [kind]: event.data })); const value = event.data as Record<string, unknown>; const items = Array.isArray(value.networks) ? value.networks : Array.isArray(value.observations) ? value.observations : []; setScanSummary(current => ({ ...current, [kind]: { ...(current[kind]), ...(kind === "wifi" ? { network_count: items.length } : { device_count: items.length }), last_scan_ms: Date.now(), duration_ms: Number(value.scan_duration_ms ?? 0) } })); setNotice(`${kind === "wifi" ? "Wi-Fi" : "蓝牙"}扫描完成。`); } catch (error) { setNotice(String(error)); } finally { setScanBusy(current => ({ ...current, [kind]: false })); } };
  const showDetails = (kind: "wifi" | "bluetooth") => { const data = scanData[kind]; if (data) setDetails({ title: kind === "wifi" ? "Wi-Fi 扫描详情" : "蓝牙扫描详情", data }); else setNotice("请先执行一次扫描。"); };

  return <main className="shell">
    <header className="topbar"><div><p>远程环境采集器</p><h1>远程环境采集器</h1><span className="device">设备 ID: {config.device_id || "正在准备"}</span></div><div className={`status ${status.connection === "Ready" ? "ok" : ""}`}>● {stateText(status.connection)}</div></header>
    {notice && <div className="notice" role="status">{notice}<button aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
    <section className="runtime"><div><h2>采集服务</h2><span>{stateText(status.connection)} · 待上传 {status.pending} · 发送中 {status.in_flight}</span></div><div className="actions"><label>上传间隔 <input type="number" min="1" max="3600" value={config.upload_interval_seconds} onChange={event => saveOptions({ upload_interval_seconds: Number(event.target.value) || 1 })}/> 秒</label><button className="primary" disabled={busy !== null} onClick={toggle}>{status.connection === "Stopped" || status.connection === "Reconnecting" ? (busy === "runtime" ? "启动中..." : "启动") : (busy === "runtime" ? "停止中..." : "停止")}</button></div></section>
    <section className="panel"><div className="heading"><div><h2>服务器</h2><span>{config.server_mode === "Multi" ? "多服务器模式" : "单服务器模式"}</span></div><button className="icon" title="新增服务器" onClick={openNew}>＋</button></div>
      <div className="options"><label><input type="radio" checked={config.server_mode === "Single"} onChange={() => saveOptions({ server_mode: "Single" })}/> 单服务器</label><label><input type="radio" checked={config.server_mode === "Multi"} onChange={() => saveOptions({ server_mode: "Multi" })}/> 多服务器</label></div>
      {config.server_profiles.length === 0 ? <p className="empty">尚未配置服务器。新增后可启动采集服务。</p> : config.server_profiles.map(server => { const live = status.servers.find(item => item.profile_id === server.id); return <article className="server" key={server.id}><div><strong>{server.name}</strong><span>{server.url}</span><small>设备 ID: {server.device_id} · {server.enabled ? "已启用" : "已禁用"} · {stateText(live?.connection ?? "Stopped")} · <b className={live?.heartbeat_alive ? "heartbeat-ok" : "heartbeat-off"}>● {live?.heartbeat_alive ? `已连接 · 心跳正常 · ${ago(live.last_heartbeat_ms, clock)}` : "未连接"}</b></small></div><div className="actions">{config.server_mode === "Single" && <label title="设为活动服务器"><input type="radio" checked={config.active_server_id === server.id} onChange={() => saveOptions({ active_server_id: server.id })}/></label>}<button className="primary" title="建立持久 WebSocket 连接" onClick={() => connect(server.id)}>{busy === `connect:${server.id}` ? "连接中..." : "连接"}</button><button title="编辑" onClick={() => edit(server)}>编辑</button><button title="测试连接" onClick={() => testServer(server.id)}>测试</button><button className="danger" title="删除" onClick={() => removeServer(server.id)}>删除</button></div></article> })}
    </section>
    <section className="grid"><Collector title="Wi-Fi" subtitle="附近网络" state={status.wifi} count={scanSummary.wifi.network_count ?? status.wifi_runtime.network_count ?? null} scan={{ ...status.wifi_runtime, ...scanSummary.wifi }} enabled={config.wifi_enabled} onToggle={(wifi_enabled) => saveOptions({ wifi_enabled })} onScan={() => scan("wifi")} onDetails={() => showDetails("wifi")} busy={scanBusy.wifi} now={clock}/><Collector title="蓝牙" subtitle="BLE + Classic Bluetooth" state={status.bluetooth} scan={{ ...status.bluetooth_runtime, ...scanSummary.bluetooth }} count={scanSummary.bluetooth.device_count ?? status.bluetooth_runtime.device_count ?? null} enabled={config.bluetooth_enabled} onToggle={(bluetooth_enabled) => saveOptions({ bluetooth_enabled })} onScan={() => scan("bluetooth")} onDetails={() => showDetails("bluetooth")} busy={scanBusy.bluetooth} now={clock}/></section>
    {dialogOpen && <div className="modal" role="dialog"><div className="dialog"><div className="heading"><h2>{editing ? "编辑服务器" : "新增服务器"}</h2><button className="icon" onClick={() => { setDialogOpen(false); setEditing(null); setForm({ name: "", url: "", device_id: "", token: "", enabled: true }); }}>×</button></div><label>名称<input value={form.name} onChange={event => setForm({ ...form, name: event.target.value })}/></label><label>WebSocket 地址<input placeholder="wss://example.com/envser/ws" value={form.url} onChange={event => setForm({ ...form, url: event.target.value })}/></label><label>设备 ID<input placeholder="服务器端已注册的 Device ID" value={form.device_id} onChange={event => setForm({ ...form, device_id: event.target.value })}/></label><label>令牌{editing && <small>留空则保留已有令牌</small>}<div className="token"><input type={showToken ? "text" : "password"} value={form.token} onChange={event => setForm({ ...form, token: event.target.value })}/><button type="button" onClick={() => setShowToken(!showToken)}>{showToken ? "隐藏" : "显示"}</button></div></label><label><input type="checkbox" checked={form.enabled} onChange={event => setForm({ ...form, enabled: event.target.checked })}/> 启用此服务器</label><div className="dialog-actions"><button onClick={() => { setDialogOpen(false); setEditing(null); setForm({ name: "", url: "", device_id: "", token: "", enabled: true }); }}>取消</button><button className="primary" onClick={saveServer}>保存</button></div></div></div>}
    {details && <div className="modal" role="dialog" onClick={() => setDetails(null)}><div className="dialog detail-dialog" onClick={event => event.stopPropagation()}><div className="heading"><h2>{details.title}</h2><button className="icon" onClick={() => setDetails(null)}>×</button></div><div className="detail-list">{detailItems(details.title.startsWith("Wi-Fi") ? "wifi" : "bluetooth", details.data).map((item, index) => <div className="detail-row" key={`${item.label}-${index}`}><span>{item.label}</span><strong>{item.value}</strong></div>)}</div></div></div>}
  </main>;
}
function Collector({ title, subtitle, state, count, scan, enabled, onToggle, onScan, onDetails, busy, now }: { title: string; subtitle: string; state: string; count: number | null; scan: Scan; enabled: boolean; onToggle: (enabled: boolean) => void; onScan: () => void; onDetails: () => void; busy: boolean; now: number }) { return <section className="panel collector"><div className="heading"><div><h2>{title}</h2><span>{subtitle}</span></div><label className="switch"><input type="checkbox" checked={enabled} onChange={event => onToggle(event.target.checked)}/><i/></label></div><strong className={state === "Ready" ? "ready" : ""}>{stateText(state)}</strong><dl><div><dt>设备数量</dt><dd>{count ?? "-"}</dd></div><div><dt>上次扫描</dt><dd>{ago(scan.last_scan_ms, now)}</dd></div><div><dt>扫描耗时</dt><dd>{scan.duration_ms ? `${(scan.duration_ms / 1000).toFixed(1)} 秒` : "-"}</dd></div><div><dt>成功 / 失败</dt><dd>{scan.successful_scans} / {scan.failed_scans}</dd></div></dl><div className="collector-actions"><button className="primary" disabled={busy} onClick={onScan}>{busy ? "扫描中..." : "扫描"}</button><button onClick={onDetails}>详情</button></div>{scan.last_error && <p className="error">{title} 扫描失败，将在下个周期重试。</p>}</section> }
createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
