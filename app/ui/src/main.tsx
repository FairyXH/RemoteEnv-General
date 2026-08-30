import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Server = { id: string; name: string; url: string; device_id: string; enabled: boolean };
type DetailItem = { label: string; value: string };
type Config = { device_id: string; server_mode: "Single" | "Multi"; active_server_id: string | null; server_profiles: Server[]; wifi_enabled: boolean; bluetooth_enabled: boolean; scan_interval_seconds: number; upload_interval_seconds: number };
type RuntimeStatus = { connection: string; collection_running: boolean; wifi: string; bluetooth: string; pending: number; in_flight: number; blocked: number; uploaded: number; failed: number; wifi_snapshot: unknown | null; bluetooth_snapshot: unknown | null; servers: Array<{ profile_id: string; connection: string; heartbeat_alive: boolean; last_heartbeat_ms: number | null; last_error?: string | null; pending: number; in_flight: number; blocked: number }>; wifi_runtime: Scan; bluetooth_runtime: Scan };
type Scan = { enabled: boolean; state: string; network_count?: number | null; device_count?: number | null; last_scan_ms: number | null; duration_ms: number | null; successful_scans: number; failed_scans: number; last_error: string | null };

const initialStatus: RuntimeStatus = { connection: "Stopped", collection_running: false, wifi: "Stopped", bluetooth: "Stopped", pending: 0, in_flight: 0, blocked: 0, uploaded: 0, failed: 0, wifi_snapshot: null, bluetooth_snapshot: null, servers: [], wifi_runtime: { enabled: false, state: "Stopped", last_scan_ms: null, duration_ms: null, successful_scans: 0, failed_scans: 0, last_error: null }, bluetooth_runtime: { enabled: false, state: "Stopped", last_scan_ms: null, duration_ms: null, successful_scans: 0, failed_scans: 0, last_error: null } };
const emptyConfig: Config = { device_id: "", server_mode: "Single", active_server_id: null, server_profiles: [], wifi_enabled: false, bluetooth_enabled: false, scan_interval_seconds: 1, upload_interval_seconds: 30 };

function ago(value: number | null, now = Date.now()) { if (!value) return "尚未扫描"; return `${Math.max(0, Math.floor((now - value) / 1000))} 秒前`; }
function detailItems(kind: "wifi" | "bluetooth", data: unknown): DetailItem[] {
  let value = data as Record<string, unknown>;
  if (value.data_type === "environment" && value.data && typeof value.data === "object") value = (value.data as Record<string, unknown>)[kind] as Record<string, unknown>;
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
  const observations = Array.isArray(value.devices) ? value.devices : Array.isArray(value.observations) ? value.observations : [];
  return observations.flatMap((item, index) => {
    const device = item as Record<string, unknown>;
    const raw = Array.isArray(device.raw_advertisement_sections) ? device.raw_advertisement_sections : Array.isArray(device.rawAdvertisementSections) ? device.rawAdvertisementSections : [];
    const rawText = raw.map(section => {
      const entry = section as Record<string, unknown>;
      return `${entry.source ?? "-"} type=${entry.ad_type ?? entry.adType ?? "-"} ${entry.data_hex ?? entry.dataHex ?? ""}`;
    }).join("; ");
    return [
      { label: `设备 ${index + 1}`, value: String(device.name ?? "未命名设备") },
      { label: "地址 / 类型", value: `${device.address ?? "-"} / ${device.mode ?? device.transport ?? "-"}` },
      { label: "信号 / 可连接", value: `${device.rssi ?? device.classicRssi ?? "-"} dBm / ${device.connectable == null ? "-" : device.connectable ? "是" : "否"}` },
      { label: "服务 UUID", value: Array.isArray(device.serviceUuids) ? device.serviceUuids.join(", ") || "-" : Array.isArray(device.service_uuids) ? device.service_uuids.join(", ") || "-" : "-" },
      { label: "RAW 广告段", value: rawText || String(device.rawHex ?? "无") },
    ];
  });
}
function stateText(value: string) { const labels: Record<string, string> = { Ready: "已连接", Running: "运行中", Reconnecting: "正在重连", Connecting: "正在连接", Authenticating: "正在认证", Blocked: "已阻止", Stopped: "已停止", Disabled: "已禁用", Starting: "正在启动", Scanning: "正在扫描", Error: "错误" }; return labels[value] ?? value; }
function heartbeatHealth(server: RuntimeStatus["servers"][number], now: number) { if (!server.last_heartbeat_ms) return { className: "heartbeat-off", text: "未连接" }; const age = Math.max(0, Math.floor((now - server.last_heartbeat_ms) / 1000)); if (age > 60) return { className: "heartbeat-critical", text: `连接严重异常 · ${age} 秒前` }; if (age > 30) return { className: "heartbeat-critical", text: `连接严重异常 · ${age} 秒前` }; if (age > 5) return { className: "heartbeat-warning", text: `连接异常 / 心跳延迟 · ${age} 秒前` }; return { className: "heartbeat-ok", text: `已连接 · 心跳正常 · ${age} 秒前` }; }

function App() {
  const [status, setStatus] = React.useState(initialStatus);
  const [config, setConfig] = React.useState(emptyConfig);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [editing, setEditing] = React.useState<Server | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [form, setForm] = React.useState({ name: "", url: "", device_id: "", token: "" });
  const [showToken, setShowToken] = React.useState(false);
  const [scanData, setScanData] = React.useState<{ wifi: unknown | null; bluetooth: unknown | null }>({ wifi: null, bluetooth: null });
  const [scanSummary, setScanSummary] = React.useState<{ wifi: Partial<Scan>; bluetooth: Partial<Scan> }>({ wifi: {}, bluetooth: {} });
  const [details, setDetails] = React.useState<{ title: string; data: unknown } | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const runtimeBusyRef = React.useRef(false);
  const serverBusyRef = React.useRef<Record<string, boolean>>({});
  const [lastServerAction, setLastServerAction] = React.useState<Record<string, number>>({});
  const serverActionAllowed = (id: string) => !serverBusyRef.current[id] && (!lastServerAction[id] || Date.now() - lastServerAction[id] >= 1000);
  const markServerAction = (id: string) => setLastServerAction(current => ({ ...current, [id]: Date.now() }));
  const [scanBusy, setScanBusy] = React.useState({ wifi: false, bluetooth: false });
  const [clock, setClock] = React.useState(Date.now());
  const [logTail, setLogTail] = React.useState("");

  React.useEffect(() => { const timer = window.setInterval(() => setClock(Date.now()), 250); return () => window.clearInterval(timer); }, []);
  React.useEffect(() => { const timer = window.setInterval(() => { invoke<string>("get_log_tail").then(setLogTail).catch(() => undefined); }, 3000); invoke<string>("get_log_tail").then(setLogTail).catch(() => undefined); return () => window.clearInterval(timer); }, []);

  const statusEventGeneration = React.useRef(0);
  const runtimeIntentRef = React.useRef<"start" | "stop" | null>(null);
  const pendingConnectionsRef = React.useRef<Record<string, boolean>>({});
  const applyStatus = React.useCallback((incoming: RuntimeStatus) => {
    const runtimeIntent = runtimeIntentRef.current;
    const pendingConnections = pendingConnectionsRef.current;
    const servers = [...incoming.servers];
    Object.keys(pendingConnections).forEach(profileId => {
      if (pendingConnections[profileId] && !servers.some(server => server.profile_id === profileId)) {
        servers.push({ profile_id: profileId, connection: "Connecting", heartbeat_alive: false, last_heartbeat_ms: null, pending: 0, in_flight: 0, blocked: 0 });
      }
    });
    const merged = { ...incoming, servers: servers.map(server => {
      if (pendingConnections[server.profile_id] && server.connection === "Stopped") {
        return { ...server, connection: "Connecting" };
      }
      if (server.connection === "Ready") delete pendingConnections[server.profile_id];
      return server;
    }) };
    if (runtimeIntent === "start" && incoming.collection_running) runtimeIntentRef.current = null;
    if (runtimeIntent === "stop" && !incoming.collection_running) runtimeIntentRef.current = null;
    if (incoming.wifi_snapshot !== null) setScanData(current => ({ ...current, wifi: incoming.wifi_snapshot }));
    if (incoming.bluetooth_snapshot !== null) setScanData(current => ({ ...current, bluetooth: incoming.bluetooth_snapshot }));
    setStatus(merged);
  }, []);
  const installStatusListener = React.useCallback(async () => {
    const generation = ++statusEventGeneration.current;
    const unlisten = await listen<RuntimeStatus>("runtime_status_changed", event => {
      if (generation === statusEventGeneration.current) applyStatus(event.payload);
    });
    if (generation !== statusEventGeneration.current) unlisten();
  }, [applyStatus]);
  const reload = React.useCallback(async () => {
    let nextStatus: RuntimeStatus;
    let nextConfig: Config;
    try { nextStatus = await invoke<RuntimeStatus>("get_runtime_status"); } catch (error) { throw new Error(`读取运行状态失败：${String(error)}`); }
    try { nextConfig = await invoke<Config>("get_desktop_config"); } catch (error) { throw new Error(`读取桌面配置失败：${String(error)}`); }
    applyStatus(nextStatus); setConfig(nextConfig);
  }, [applyStatus]);
  React.useEffect(() => { let active = true; (async () => { await installStatusListener(); if (active) await reload(); })().catch(error => setNotice(String(error))); return () => { active = false; statusEventGeneration.current += 1; }; }, [installStatusListener, reload]);
  React.useEffect(() => { const blocked = status.servers.find(server => server.connection === "Blocked"); if (blocked) { setNotice(`服务器连接被拒绝（${blocked.profile_id}）：${blocked.last_error ?? "未提供具体原因"}`); } }, [status.servers]);
  const saveOptions = async (next: Partial<Config>) => { try { const result = await invoke<Config>("set_runtime_options", { serverMode: next.server_mode ?? config.server_mode, activeServerId: Object.prototype.hasOwnProperty.call(next, "active_server_id") ? next.active_server_id : config.active_server_id, wifiEnabled: next.wifi_enabled ?? config.wifi_enabled, bluetoothEnabled: next.bluetooth_enabled ?? config.bluetooth_enabled, scanIntervalSeconds: next.scan_interval_seconds ?? config.scan_interval_seconds, uploadIntervalSeconds: next.upload_interval_seconds ?? config.upload_interval_seconds }); setConfig(result); } catch (error) { setNotice(`保存采集服务设置失败：${String(error)}`); } };
  const openNew = () => { setEditing(null); setDialogOpen(true); setForm({ name: "", url: "", device_id: "", token: "" }); setShowToken(false); };
  const edit = (server: Server) => { setEditing(server); setDialogOpen(true); setForm({ name: server.name, url: server.url, device_id: server.device_id, token: "" }); setShowToken(false); };
  const saveServer = async () => { try { const result = await invoke<Config>("save_server_profile", { input: { id: editing?.id, ...form } }); setConfig(result); setDialogOpen(false); setEditing(null); setNotice("服务器配置已保存。"); } catch (error) { setNotice(String(error)); } };
  const testServer = async (id: string) => { if (busy) return; setBusy(`test:${id}`); setNotice("正在测试连接..."); try { const result = await invoke<{ message: string }>("test_server_profile", { id }); setNotice(result.message); } catch (error) { setNotice(String(error)); } finally { setBusy(null); } };
  const removeServer = async (id: string) => { if (!confirm("确定删除此服务器配置吗？")) return; try { setConfig(await invoke<Config>("delete_server_profile", { id })); } catch { setNotice("删除服务器配置失败。") } };
  const toggle = async () => { if (runtimeBusyRef.current) return; runtimeBusyRef.current = true; const shouldStart = !status.collection_running; runtimeIntentRef.current = shouldStart ? "start" : "stop"; setBusy("runtime"); try { const next = await invoke<RuntimeStatus>(shouldStart ? "start_runtime" : "stop_runtime"); applyStatus(next); setNotice(shouldStart ? "采集服务已启动。" : "采集服务已停止，服务器连接保持独立运行。"); runtimeIntentRef.current = null; } catch (error) { runtimeIntentRef.current = null; setNotice(`采集服务操作失败：${String(error)}`); } finally { runtimeBusyRef.current = false; setBusy(null); } };
  const connect = async (id: string) => { if (!serverActionAllowed(id)) return; pendingConnectionsRef.current[id] = true; serverBusyRef.current[id] = true; markServerAction(id); setBusy(`connect:${id}`); setNotice("正在建立持久连接..."); try { const result = await invoke<RuntimeStatus>("connect_server_profile", { id }); applyStatus(result); setNotice("服务器连接已提交，等待认证结果。"); } catch (error) { delete pendingConnectionsRef.current[id]; setNotice(`服务器连接失败：${String(error)}`); } finally { serverBusyRef.current[id] = false; setBusy(null); } };
  const disconnect = async (id: string) => { if (!serverActionAllowed(id)) return; delete pendingConnectionsRef.current[id]; serverBusyRef.current[id] = true; markServerAction(id); setBusy(`disconnect:${id}`); setNotice("正在断开服务器..."); try { const result = await invoke<RuntimeStatus>("disconnect_server_profile", { id }); applyStatus(result); setConfig(current => ({ ...current, active_server_id: current.active_server_id === id ? null : current.active_server_id, server_profiles: current.server_profiles.map(server => server.id === id ? { ...server, enabled: false } : server) })); setNotice("服务器已断开。"); } catch (error) { setNotice(`服务器断开失败：${String(error)}`); } finally { serverBusyRef.current[id] = false; setBusy(null); } };
  const toggleServer = async (server: Server) => { if (serverBusyRef.current[server.id] || !serverActionAllowed(server.id)) return; serverBusyRef.current[server.id] = true; markServerAction(server.id); setBusy(`toggle:${server.id}`); try { const result = await invoke<RuntimeStatus>("set_server_enabled", { id: server.id, enabled: !server.enabled }); applyStatus(result); setConfig(current => ({ ...current, active_server_id: !server.enabled ? server.id : current.active_server_id === server.id ? null : current.active_server_id, server_profiles: current.server_profiles.map(item => item.id === server.id ? { ...item, enabled: !server.enabled } : item) })); setNotice(!server.enabled ? "服务器已启用，采集服务状态不变。" : "服务器已断开。"); } catch (error) { setNotice(`服务器切换失败：${String(error)}`); } finally { serverBusyRef.current[server.id] = false; setBusy(null); } };
  const scan = async (kind: "wifi" | "bluetooth") => { if (scanBusy[kind]) return; setScanBusy(current => ({ ...current, [kind]: true })); setNotice(`正在扫描${kind === "wifi" ? " Wi-Fi" : "蓝牙"}...`); try { const event = await invoke<{ data: unknown }>(kind === "wifi" ? "scan_wifi_now" : "scan_bluetooth_now"); setScanData(current => ({ ...current, [kind]: event.data })); const value = event.data as Record<string, unknown>; const items = Array.isArray(value.networks) ? value.networks : Array.isArray(value.observations) ? value.observations : []; setScanSummary(current => ({ ...current, [kind]: { ...(current[kind]), ...(kind === "wifi" ? { network_count: items.length } : { device_count: items.length }), last_scan_ms: Date.now(), duration_ms: Number(value.scan_duration_ms ?? 0) } })); setNotice(`${kind === "wifi" ? "Wi-Fi" : "蓝牙"}扫描完成。`); } catch (error) { setNotice(String(error)); } finally { setScanBusy(current => ({ ...current, [kind]: false })); } };
  const showDetails = (kind: "wifi" | "bluetooth") => { const data = scanData[kind] ?? (kind === "wifi" ? status.wifi_snapshot : status.bluetooth_snapshot); if (data) setDetails({ title: kind === "wifi" ? "Wi-Fi 扫描详情" : "蓝牙扫描详情", data }); else setNotice("尚未获得扫描数据，请先启动采集服务。"); };

  return <main className="shell">
    <header className="topbar"><div><p>远程环境采集器</p><h1>远程环境采集器</h1><span className="device">设备 ID: {config.device_id || "正在准备"}</span></div><div className={`status ${status.connection === "Ready" ? "ok" : ""}`}>● {stateText(status.connection)}</div></header>
    <p className="hint">自启动托盘模式：给程序快捷方式或启动命令追加参数 <code>--tray</code>，例如 <code>RemoteEnvCollector.exe --tray</code>。以 <code>--tray</code> 启动时主窗口保持隐藏，程序驻留系统托盘并自动开始采集；需要配置界面时点击托盘菜单「打开主窗口」。</p>
    {notice && <div className="notice" role="status">{notice}<button aria-label="关闭提示" onClick={() => setNotice(null)}>×</button></div>}
    <section className="runtime"><div><h2>采集服务</h2><span>{status.collection_running ? "运行中" : "已停止"} · 服务器连接独立管理 · 待上传 {status.pending} · 发送中 {status.in_flight} · 组合包成功 {status.uploaded} · 失败 {status.failed}</span></div><div className="actions"><label>上传间隔 <input type="number" min="1" max="3600" value={config.upload_interval_seconds} onChange={event => saveOptions({ upload_interval_seconds: Number(event.target.value) || 1 })}/> 秒</label><button className="primary" disabled={busy !== null} onClick={toggle}>{status.collection_running ? (busy === "runtime" ? "停止中..." : "停止采集") : (busy === "runtime" ? "启动中..." : "开始采集")}</button></div></section>
    <section className="panel"><div className="heading"><div><h2>服务器</h2><span>{config.server_mode === "Multi" ? "多服务器模式" : "单服务器模式"}</span></div><button className="icon" title="新增服务器" onClick={openNew}>＋</button></div>
      <div className="options"><label><input type="radio" checked={config.server_mode === "Single"} onChange={() => saveOptions({ server_mode: "Single" })}/> 单服务器</label><label><input type="radio" checked={config.server_mode === "Multi"} onChange={() => saveOptions({ server_mode: "Multi" })}/> 多服务器</label></div>
      {config.server_profiles.length === 0 ? <p className="empty">尚未配置服务器。新增后可启动采集服务。</p> : config.server_profiles.map(server => { const live = status.servers.find(item => item.profile_id === server.id); const connected = live?.connection === "Ready"; const selected = server.enabled || connected || ["Connecting", "Authenticating", "Reconnecting"].includes(live?.connection ?? ""); const heartbeat = live ? heartbeatHealth(live, clock) : { className: "heartbeat-off", text: "未连接" }; return <article className="server" key={server.id}><div><strong>{server.name}</strong><span>{server.url}</span><small>设备 ID: {server.device_id} · {stateText(live?.connection ?? "Stopped")} · <b className={heartbeat.className}>● {heartbeat.text}</b>{live?.last_error && <em className="connection-error"> · {live.last_error}</em>}</small></div><div className="actions">{config.server_mode === "Single" ? <><label title="设为活动服务器"><input type="radio" checked={config.active_server_id === server.id} onChange={() => saveOptions({ active_server_id: server.id })}/></label><button className="primary" disabled={!serverActionAllowed(server.id)} title={connected ? "断开服务器" : "建立持久 WebSocket 连接"} onClick={() => connected ? disconnect(server.id) : connect(server.id)}>{busy === `connect:${server.id}` ? "连接中..." : busy === `disconnect:${server.id}` || busy === `toggle:${server.id}` ? "处理中..." : connected ? "断开" : "连接"}</button></> : <label className="server-check"><input type="checkbox" checked={selected} disabled={!serverActionAllowed(server.id)} onChange={() => toggleServer(server)}/> {selected ? "已连接/连接中" : "启用"}</label>}<button title="编辑" onClick={() => edit(server)}>编辑</button><button title="测试连接" onClick={() => testServer(server.id)}>测试</button><button className="danger" title="删除" onClick={() => removeServer(server.id)}>删除</button></div></article> })}
    </section>
    <section className="grid"><Collector title="Wi-Fi" subtitle="附近网络" state={status.wifi} count={scanSummary.wifi.network_count ?? status.wifi_runtime.network_count ?? (status.wifi_snapshot && Array.isArray((status.wifi_snapshot as Record<string, unknown>).networks) ? ((status.wifi_snapshot as Record<string, unknown>).networks as unknown[]).length : null)} scan={{ ...status.wifi_runtime, ...scanSummary.wifi }} enabled={config.wifi_enabled} onToggle={(wifi_enabled) => saveOptions({ wifi_enabled })} onScan={() => scan("wifi")} onDetails={() => showDetails("wifi")} busy={scanBusy.wifi} now={clock}/><Collector title="蓝牙" subtitle="BLE + Classic Bluetooth" state={status.bluetooth} count={scanSummary.bluetooth.device_count ?? status.bluetooth_runtime.device_count ?? (status.bluetooth_snapshot && typeof status.bluetooth_snapshot === "object" && Array.isArray((status.bluetooth_snapshot as Record<string, unknown>).devices) ? ((status.bluetooth_snapshot as Record<string, unknown>).devices as unknown[]).length : null)} scan={{ ...status.bluetooth_runtime, ...scanSummary.bluetooth }} enabled={config.bluetooth_enabled} onToggle={(bluetooth_enabled) => saveOptions({ bluetooth_enabled })} onScan={() => scan("bluetooth")} onDetails={() => showDetails("bluetooth")} busy={scanBusy.bluetooth} now={clock}/></section>
    {dialogOpen && <div className="modal" role="dialog"><div className="dialog"><div className="heading"><h2>{editing ? "编辑服务器" : "新增服务器"}</h2><button className="icon" onClick={() => { setDialogOpen(false); setEditing(null); setForm({ name: "", url: "", device_id: "", token: "" }); }}>×</button></div><label>名称<input value={form.name} onChange={event => setForm({ ...form, name: event.target.value })}/></label><label>WebSocket 地址<input placeholder="wss://example.com/envser/ws" value={form.url} onChange={event => setForm({ ...form, url: event.target.value })}/></label><label>设备 ID<input placeholder="服务器端已注册的 Device ID" value={form.device_id} onChange={event => setForm({ ...form, device_id: event.target.value })}/></label><label>令牌{editing && <small>留空则保留已有令牌</small>}<div className="token"><input type={showToken ? "text" : "password"} value={form.token} onChange={event => setForm({ ...form, token: event.target.value })}/><button type="button" onClick={() => setShowToken(!showToken)}>{showToken ? "隐藏" : "显示"}</button></div></label><div className="dialog-actions"><button onClick={() => { setDialogOpen(false); setEditing(null); setForm({ name: "", url: "", device_id: "", token: "" }); }}>取消</button><button className="primary" onClick={saveServer}>保存</button></div></div></div>}
    {details && <div className="modal" role="dialog" onClick={() => setDetails(null)}><div className="dialog detail-dialog" onClick={event => event.stopPropagation()}><div className="heading"><h2>{details.title}</h2><button className="icon" onClick={() => setDetails(null)}>×</button></div><div className="detail-list">{detailItems(details.title.startsWith("Wi-Fi") ? "wifi" : "bluetooth", details.data).map((item, index) => <div className="detail-row" key={`${item.label}-${index}`}><span>{item.label}</span><strong>{item.value}</strong></div>)}</div></div></div>}
  </main>;
}
function Collector({ title, subtitle, state, count, scan, enabled, onToggle, onScan, onDetails, busy, now }: { title: string; subtitle: string; state: string; count: number | null; scan: Scan; enabled: boolean; onToggle: (enabled: boolean) => void; onScan: () => void; onDetails: () => void; busy: boolean; now: number }) { return <section className="panel collector"><div className="heading"><div><h2>{title}</h2><span>{subtitle}</span></div><label className="switch"><input type="checkbox" checked={enabled} onChange={event => onToggle(event.target.checked)}/><i/></label></div><strong className={state === "Ready" ? "ready" : ""}>{stateText(state)}</strong><dl><div><dt>设备数量</dt><dd>{count ?? "-"}</dd></div><div><dt>上次扫描</dt><dd>{ago(scan.last_scan_ms, now)}</dd></div><div><dt>扫描耗时</dt><dd>{scan.duration_ms ? `${(scan.duration_ms / 1000).toFixed(1)} 秒` : "-"}</dd></div><div><dt>成功 / 失败</dt><dd>{scan.successful_scans} / {scan.failed_scans}</dd></div></dl><div className="collector-actions"><button className="primary" disabled={busy} onClick={onScan}>{busy ? "扫描中..." : "扫描"}</button><button onClick={onDetails}>详情</button></div>{scan.last_error && <p className="error">{title} 扫描失败，将在下个周期重试。</p>}</section> }
createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
