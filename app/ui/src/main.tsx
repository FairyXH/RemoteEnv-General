import React from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type Server = {
  id: string;
  name: string;
  url: string;
  device_id: string;
  enabled: boolean;
};
type DetailItem = { label: string; value: string };
type Config = {
  device_id: string;
  server_mode: "Single" | "Multi";
  active_server_id: string | null;
  server_profiles: Server[];
  scan_interval_seconds: number;
  upload_interval_seconds: number;
};
type CollectorKind = "wifi" | "bluetooth" | "cell" | "gps" | "gnss";
type RuntimeStatus = {
  connection: string;
  collection_running: boolean;
  wifi: string;
  bluetooth: string;
  pending: number;
  in_flight: number;
  blocked: number;
  uploaded: number;
  failed: number;
  wifi_snapshot: unknown | null;
  bluetooth_snapshot: unknown | null;
  cell_snapshot: unknown | null;
  gps_snapshot: unknown | null;
  gnss_snapshot: unknown | null;
  servers: Array<{
    profile_id: string;
    connection: string;
    heartbeat_alive: boolean;
    last_heartbeat_ms: number | null;
    last_error?: string | null;
    next_retry_at_ms?: number | null;
    pending: number;
    in_flight: number;
    blocked: number;
  }>;
  wifi_runtime: Scan;
  bluetooth_runtime: Scan;
};
type Scan = {
  enabled: boolean;
  state: string;
  network_count?: number | null;
  device_count?: number | null;
  last_scan_ms: number | null;
  duration_ms: number | null;
  successful_scans: number;
  failed_scans: number;
  last_error: string | null;
};
type StateInfo = {
  config_db_path: string;
  config_db_bytes: number;
  state_cache_db_path: string;
  state_cache_db_bytes: number;
};
type PersistenceSettings = {
  is_android: boolean;
  foreground_enabled: boolean;
  auto_start_enabled: boolean;
  hide_from_recents: boolean;
  accessibility_enabled: boolean;
  battery_optimization_ignored: boolean;
  root_enabled: boolean;
  root_available: boolean;
  device_admin_active: boolean;
  device_owner_active: boolean;
  profile_owner_active: boolean;
  dhizuku_compat_enabled: boolean;
  dhizuku_supported: boolean;
  android_api_level: number;
  background_location_granted: boolean;
  location_enabled: boolean;
  device_owner_command: string;
};
type DhizukuApp = {
  package_name: string;
  label: string;
  uid: number;
  allowed: boolean;
};

const initialStatus: RuntimeStatus = {
  connection: "Stopped",
  collection_running: false,
  wifi: "Stopped",
  bluetooth: "Stopped",
  pending: 0,
  in_flight: 0,
  blocked: 0,
  uploaded: 0,
  failed: 0,
  wifi_snapshot: null,
  bluetooth_snapshot: null,
  cell_snapshot: null,
  gps_snapshot: null,
  gnss_snapshot: null,
  servers: [],
  wifi_runtime: {
    enabled: false,
    state: "Stopped",
    last_scan_ms: null,
    duration_ms: null,
    successful_scans: 0,
    failed_scans: 0,
    last_error: null,
  },
  bluetooth_runtime: {
    enabled: false,
    state: "Stopped",
    last_scan_ms: null,
    duration_ms: null,
    successful_scans: 0,
    failed_scans: 0,
    last_error: null,
  },
};
const emptyConfig: Config = {
  device_id: "",
  server_mode: "Single",
  active_server_id: null,
  server_profiles: [],
  scan_interval_seconds: 1,
  upload_interval_seconds: 30,
};

function ago(value: number | null, now = Date.now()) {
  if (!value) return "尚未扫描";
  return `${Math.max(0, Math.floor((now - value) / 1000))} 秒前`;
}
function detailItems(kind: CollectorKind, data: unknown): DetailItem[] {
  let value = data as Record<string, unknown>;
  if (
    value.data_type === "environment" &&
    value.data &&
    typeof value.data === "object"
  )
    value = (value.data as Record<string, unknown>)[kind] as Record<
      string,
      unknown
    >;
  if (kind === "wifi") {
    const networks = Array.isArray(value.networks) ? value.networks : [];
    return networks.flatMap((item, index) => {
      const network = item as Record<string, unknown>;
      return [
        {
          label: `网络 ${index + 1}`,
          value: String(network.ssid ?? "隐藏网络"),
        },
        {
          label: "BSSID / 信号",
          value: `${network.bssid ?? "-"} / ${network.signal_strength_dbm ?? "-"} dBm`,
        },
        {
          label: "频段 / 信道",
          value: `${network.band ?? "-"} / ${network.channel ?? "-"}`,
        },
        {
          label: "频率 / 质量",
          value: `${network.frequency_mhz ?? "-"} MHz / ${network.signal_percent ?? "-"}%`,
        },
        { label: "接口", value: String(network.interface_id ?? "-") },
      ];
    });
  }
  if (kind === "cell") {
    const cells = [
      ...(value.serving && typeof value.serving === "object"
        ? [value.serving]
        : []),
      ...(Array.isArray(value.neighbors) ? value.neighbors : []),
    ];
    return cells.flatMap((item, index) => {
      const cell = item as Record<string, unknown>;
      return [
        {
          label:
            index === 0 && value.serving
              ? "服务基站"
              : `邻区基站 ${index + (value.serving ? 0 : 1)}`,
          value: `${cell.type ?? cell.radio ?? "-"} · ${cell.registered ? "已注册" : "邻区"}`,
        },
        {
          label: "MCC / MNC",
          value: `${cell.mcc ?? "-"} / ${cell.mnc ?? "-"}`,
        },
        {
          label: "TAC/LAC / CID / PCI",
          value: `${cell.tac ?? cell.lac ?? "-"} / ${cell.cid ?? "-"} / ${cell.pci ?? "-"}`,
        },
        {
          label: "ARFCN / 信号",
          value: `${cell.arfcn ?? "-"} / ${cell.signal_dbm ?? cell.rssi ?? "-"} dBm`,
        },
        {
          label: "RSRP / RSRQ / ASU",
          value: `${cell.rsrp ?? "-"} / ${cell.rsrq ?? "-"} / ${cell.asu ?? "-"}`,
        },
      ];
    });
  }
  if (kind === "gps") {
    const points = Array.isArray(value.points) ? value.points : [];
    const locations = points.length
      ? points
      : value.latitude != null
        ? [value]
        : [];
    return locations.flatMap((item, index) => {
      const point = item as Record<string, unknown>;
      return [
        {
          label: `定位 ${index + 1}`,
          value: `${point.latitude ?? "-"}, ${point.longitude ?? "-"}`,
        },
        {
          label: "提供方 / 时间",
          value: `${point.provider ?? value.provider ?? "-"} / ${point.fix_at ?? value.fix_at ?? "-"}`,
        },
        {
          label: "精度 / 海拔",
          value: `${point.accuracy_m ?? value.accuracy_m ?? "-"} m / ${point.altitude_m ?? value.altitude_m ?? "-"} m`,
        },
        {
          label: "速度 / 方位",
          value: `${point.speed_mps ?? value.speed_mps ?? "-"} m/s / ${point.bearing_deg ?? value.bearing_deg ?? "-"}°`,
        },
        {
          label: "质量 / 模拟位置",
          value: `${point.fix_quality ?? value.fix_quality ?? "-"} / ${(point.mocked ?? value.mocked) == null ? "-" : (point.mocked ?? value.mocked) ? "是" : "否"}`,
        },
      ];
    });
  }
  if (kind === "gnss") {
    const satellites = Array.isArray(value.satellites) ? value.satellites : [];
    return satellites.flatMap((item, index) => {
      const satellite = item as Record<string, unknown>;
      return [
        {
          label: `卫星 ${index + 1}`,
          value: `${satellite.type ?? satellite.constellation ?? "-"} · SVID ${satellite.svid ?? "-"}`,
        },
        {
          label: "信噪比 / 用于定位",
          value: `${satellite.cn0_dbhz ?? "-"} dB-Hz / ${satellite.used_in_fix ? "是" : "否"}`,
        },
        {
          label: "高度角 / 方位角",
          value: `${satellite.elevation_deg ?? "-"}° / ${satellite.azimuth_deg ?? "-"}°`,
        },
        {
          label: "载波频率",
          value: `${satellite.carrier_frequency_hz ?? "-"} Hz`,
        },
      ];
    });
  }
  const observations = Array.isArray(value.devices)
    ? value.devices
    : Array.isArray(value.observations)
      ? value.observations
      : [];
  return observations.flatMap((item, index) => {
    const device = item as Record<string, unknown>;
    const raw = Array.isArray(device.raw_advertisement_sections)
      ? device.raw_advertisement_sections
      : Array.isArray(device.rawAdvertisementSections)
        ? device.rawAdvertisementSections
        : [];
    const rawText = raw
      .map((section) => {
        const entry = section as Record<string, unknown>;
        return `${entry.source ?? "-"} type=${entry.ad_type ?? entry.adType ?? "-"} ${entry.data_hex ?? entry.dataHex ?? ""}`;
      })
      .join("; ");
    return [
      {
        label: `设备 ${index + 1}`,
        value: String(device.name ?? "未命名设备"),
      },
      {
        label: "地址 / 类型",
        value: `${device.address ?? "-"} / ${device.mode ?? device.transport ?? "-"}`,
      },
      {
        label: "信号 / 可连接",
        value: `${device.rssi ?? device.classicRssi ?? "-"} dBm / ${device.connectable == null ? "-" : device.connectable ? "是" : "否"}`,
      },
      {
        label: "服务 UUID",
        value: Array.isArray(device.serviceUuids)
          ? device.serviceUuids.join(", ") || "-"
          : Array.isArray(device.service_uuids)
            ? device.service_uuids.join(", ") || "-"
            : "-",
      },
      { label: "RAW 广告段", value: rawText || String(device.rawHex ?? "无") },
    ];
  });
}
const collectorNames: Record<CollectorKind, string> = {
  wifi: "Wi-Fi",
  bluetooth: "蓝牙",
  cell: "基站",
  gps: "GPS",
  gnss: "GNSS",
};
function itemCount(kind: CollectorKind, data: unknown): number | null {
  if (!data || typeof data !== "object") return null;
  const value = data as Record<string, unknown>;
  if (kind === "wifi")
    return Array.isArray(value.networks) ? value.networks.length : 0;
  if (kind === "bluetooth")
    return Array.isArray(value.devices)
      ? value.devices.length
      : Array.isArray(value.observations)
        ? value.observations.length
        : 0;
  if (kind === "cell")
    return (
      (value.serving && typeof value.serving === "object" ? 1 : 0) +
      (Array.isArray(value.neighbors) ? value.neighbors.length : 0)
    );
  if (kind === "gps")
    return value.latitude != null
      ? 1
      : Array.isArray(value.points)
        ? value.points.length
        : 0;
  return Array.isArray(value.satellites) ? value.satellites.length : 0;
}
function stateText(value: string) {
  const labels: Record<string, string> = {
    Ready: "已连接",
    Running: "运行中",
    Reconnecting: "正在重连",
    Connecting: "正在连接",
    Authenticating: "正在认证",
    Blocked: "已暂停",
    Stopped: "已停止",
    Disabled: "已禁用",
    Starting: "正在启动",
    Scanning: "正在扫描",
    Error: "错误",
  };
  return labels[value] ?? value;
}
function heartbeatHealth(
  server: RuntimeStatus["servers"][number],
  now: number,
) {
  if (!server.last_heartbeat_ms)
    return { className: "heartbeat-off", text: "未连接" };
  const age = Math.max(0, Math.floor((now - server.last_heartbeat_ms) / 1000));
  if (age > 60)
    return {
      className: "heartbeat-critical",
      text: `连接严重异常 · ${age} 秒前`,
    };
  if (age > 30)
    return {
      className: "heartbeat-critical",
      text: `连接严重异常 · ${age} 秒前`,
    };
  if (age > 5)
    return {
      className: "heartbeat-warning",
      text: `连接异常 / 心跳延迟 · ${age} 秒前`,
    };
  return { className: "heartbeat-ok", text: `已连接 · 心跳正常 · ${age} 秒前` };
}

function App() {
  const [status, setStatus] = React.useState(initialStatus);
  const [config, setConfig] = React.useState(emptyConfig);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [editing, setEditing] = React.useState<Server | null>(null);
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [form, setForm] = React.useState({
    name: "",
    url: "",
    device_id: "",
    token: "",
  });
  const [showToken, setShowToken] = React.useState(false);
  const [scanData, setScanData] = React.useState<
    Record<CollectorKind, unknown | null>
  >({ wifi: null, bluetooth: null, cell: null, gps: null, gnss: null });
  const [scanSummary, setScanSummary] = React.useState<
    Record<CollectorKind, Partial<Scan>>
  >({ wifi: {}, bluetooth: {}, cell: {}, gps: {}, gnss: {} });
  const [details, setDetails] = React.useState<{
    kind: CollectorKind;
    title: string;
    data: unknown;
  } | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const runtimeBusyRef = React.useRef(false);
  const serverBusyRef = React.useRef<Record<string, boolean>>({});
  const configBusyRef = React.useRef(false);
  const [configBusy, setConfigBusy] = React.useState(false);
  const serverActionAllowed = (id: string) =>
    !configBusyRef.current && !serverBusyRef.current[id];
  const beginConfigChange = () => {
    if (configBusyRef.current) return false;
    configBusyRef.current = true;
    setConfigBusy(true);
    return true;
  };
  const endConfigChange = () => {
    configBusyRef.current = false;
    setConfigBusy(false);
  };
  const [scanBusy, setScanBusy] = React.useState<
    Record<CollectorKind, boolean>
  >({ wifi: false, bluetooth: false, cell: false, gps: false, gnss: false });
  const [clock, setClock] = React.useState(Date.now());
  const [logTail, setLogTail] = React.useState("");
  const [stateInfo, setStateInfo] = React.useState<StateInfo | null>(null);
  const [cleaning, setCleaning] = React.useState(false);
  const [persistence, setPersistence] =
    React.useState<PersistenceSettings | null>(null);
  const [dhizukuPage, setDhizukuPage] = React.useState(false);
  const [dhizukuApps, setDhizukuApps] = React.useState<DhizukuApp[]>([]);
  const [dhizukuBusy, setDhizukuBusy] = React.useState<string | null>(null);
  const formatBytes = (bytes: number) =>
    bytes < 1024
      ? `${bytes} B`
      : bytes < 1024 * 1024
        ? `${(bytes / 1024).toFixed(1)} KB`
        : `${(bytes / 1024 / 1024).toFixed(2)} MB`;
  const refreshStateInfo = React.useCallback(() => {
    invoke<StateInfo>("get_state_info")
      .then(setStateInfo)
      .catch(() => undefined);
  }, []);
  React.useEffect(() => {
    refreshStateInfo();
    const timer = window.setInterval(refreshStateInfo, 15000);
    return () => window.clearInterval(timer);
  }, [refreshStateInfo]);
  const cleanupCache = async () => {
    if (cleaning) return;
    setCleaning(true);
    try {
      const result = await invoke<{
        removed_rows: number;
        state_cache_db_bytes: number;
      }>("cleanup_state_cache");
      setNotice(
        `状态缓存清理完成：删除 ${result.removed_rows} 条已完成记录，当前缓存大小 ${formatBytes(result.state_cache_db_bytes)}。`,
      );
      refreshStateInfo();
    } catch (error) {
      setNotice(`状态缓存清理失败：${String(error)}`);
    } finally {
      setCleaning(false);
    }
  };

  React.useEffect(() => {
    const timer = window.setInterval(() => setClock(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, []);
  React.useEffect(() => {
    const timer = window.setInterval(() => {
      invoke<string>("get_log_tail")
        .then(setLogTail)
        .catch(() => undefined);
    }, 3000);
    invoke<string>("get_log_tail")
      .then(setLogTail)
      .catch(() => undefined);
    return () => window.clearInterval(timer);
  }, []);

  const statusEventGeneration = React.useRef(0);
  const runtimeIntentRef = React.useRef<"start" | "stop" | null>(null);
  const pendingConnectionsRef = React.useRef<Record<string, boolean>>({});
  const applyStatus = React.useCallback((incoming: RuntimeStatus) => {
    const runtimeIntent = runtimeIntentRef.current;
    const pendingConnections = pendingConnectionsRef.current;
    const servers = [...incoming.servers];
    Object.keys(pendingConnections).forEach((profileId) => {
      if (
        pendingConnections[profileId] &&
        !servers.some((server) => server.profile_id === profileId)
      ) {
        servers.push({
          profile_id: profileId,
          connection: "Connecting",
          heartbeat_alive: false,
          last_heartbeat_ms: null,
          pending: 0,
          in_flight: 0,
          blocked: 0,
        });
      }
    });
    const merged = {
      ...incoming,
      servers: servers.map((server) => {
        if (
          pendingConnections[server.profile_id] &&
          server.connection === "Stopped"
        ) {
          return { ...server, connection: "Connecting" };
        }
        if (["Connecting", "Authenticating", "Reconnecting", "Ready"].includes(server.connection))
          delete pendingConnections[server.profile_id];
        return server;
      }),
    };
    if (runtimeIntent === "start" && incoming.collection_running)
      runtimeIntentRef.current = null;
    if (runtimeIntent === "stop" && !incoming.collection_running)
      runtimeIntentRef.current = null;
    if (incoming.wifi_snapshot !== null)
      setScanData((current) => ({ ...current, wifi: incoming.wifi_snapshot }));
    if (incoming.bluetooth_snapshot !== null)
      setScanData((current) => ({
        ...current,
        bluetooth: incoming.bluetooth_snapshot,
      }));
    if (incoming.cell_snapshot !== null)
      setScanData((current) => ({ ...current, cell: incoming.cell_snapshot }));
    if (incoming.gps_snapshot !== null)
      setScanData((current) => ({ ...current, gps: incoming.gps_snapshot }));
    if (incoming.gnss_snapshot !== null)
      setScanData((current) => ({ ...current, gnss: incoming.gnss_snapshot }));
    setStatus(merged);
  }, []);
  const installStatusListener = React.useCallback(async () => {
    const generation = ++statusEventGeneration.current;
    const unlisten = await listen<RuntimeStatus>(
      "runtime_status_changed",
      (event) => {
        if (generation === statusEventGeneration.current)
          applyStatus(event.payload);
      },
    );
    if (generation !== statusEventGeneration.current) unlisten();
  }, [applyStatus]);
  const reload = React.useCallback(async () => {
    let nextStatus: RuntimeStatus;
    let nextConfig: Config;
    try {
      nextStatus = await invoke<RuntimeStatus>("get_runtime_status");
    } catch (error) {
      throw new Error(`读取运行状态失败：${String(error)}`);
    }
    try {
      nextConfig = await invoke<Config>("get_desktop_config");
    } catch (error) {
      throw new Error(`读取桌面配置失败：${String(error)}`);
    }
    applyStatus(nextStatus);
    setConfig(nextConfig);
    invoke<PersistenceSettings>("get_persistence_settings")
      .then(setPersistence)
      .catch(() => undefined);
  }, [applyStatus]);
  React.useEffect(() => {
    let active = true;
    (async () => {
      await installStatusListener();
      if (active) await reload();
    })().catch((error) => setNotice(String(error)));
    return () => {
      active = false;
      statusEventGeneration.current += 1;
    };
  }, [installStatusListener, reload]);
  React.useEffect(() => {
    const refreshPersistence = () => {
      invoke<PersistenceSettings>("get_persistence_settings")
        .then(setPersistence)
        .catch(() => undefined);
    };
    window.addEventListener("focus", refreshPersistence);
    document.addEventListener("visibilitychange", refreshPersistence);
    return () => {
      window.removeEventListener("focus", refreshPersistence);
      document.removeEventListener("visibilitychange", refreshPersistence);
    };
  }, []);
  React.useEffect(() => {
    const blocked = status.servers.find(
      (server) => server.connection === "Blocked",
    );
    if (blocked) {
      setNotice(
        `服务器已暂停（${blocked.profile_id}）：${blocked.last_error ?? "未提供具体原因"}`,
      );
    }
  }, [status.servers]);
  const saveOptions = async (next: Partial<Config>) => {
    if (!beginConfigChange()) return;
    const previous = config;
    setConfig((current) => ({ ...current, ...next }));
    try {
      const result = await invoke<Config>("set_runtime_options", {
        serverMode: next.server_mode ?? config.server_mode,
        activeServerId: Object.prototype.hasOwnProperty.call(
          next,
          "active_server_id",
        )
          ? next.active_server_id
          : config.active_server_id,
        scanIntervalSeconds:
          next.scan_interval_seconds ?? config.scan_interval_seconds,
        uploadIntervalSeconds:
          next.upload_interval_seconds ?? config.upload_interval_seconds,
      });
      setConfig(result);
    } catch (error) {
      setConfig(previous);
      setNotice(`保存采集服务设置失败：${String(error)}`);
    } finally {
      endConfigChange();
    }
  };
  const openNew = () => {
    setEditing(null);
    setDialogOpen(true);
    setForm({ name: "", url: "", device_id: "", token: "" });
    setShowToken(false);
  };
  const edit = (server: Server) => {
    setEditing(server);
    setDialogOpen(true);
    setForm({
      name: server.name,
      url: server.url,
      device_id: server.device_id,
      token: "",
    });
    setShowToken(false);
  };
  const saveServer = async () => {
    try {
      const result = await invoke<Config>("save_server_profile", {
        input: { id: editing?.id, ...form },
      });
      setConfig(result);
      setDialogOpen(false);
      setEditing(null);
      setNotice("服务器配置已保存。");
    } catch (error) {
      setNotice(String(error));
    }
  };
  const testServer = async (id: string) => {
    if (busy) return;
    setBusy(`test:${id}`);
    setNotice("正在测试连接...");
    try {
      const result = await invoke<{ message: string }>("test_server_profile", {
        id,
      });
      setNotice(result.message);
    } catch (error) {
      setNotice(String(error));
    } finally {
      setBusy(null);
    }
  };
  const removeServer = async (id: string) => {
    if (!confirm("确定删除此服务器配置吗？")) return;
    try {
      setConfig(await invoke<Config>("delete_server_profile", { id }));
    } catch {
      setNotice("删除服务器配置失败。");
    }
  };
  const toggle = async () => {
    if (runtimeBusyRef.current) return;
    runtimeBusyRef.current = true;
    const shouldStart = !status.collection_running;
    runtimeIntentRef.current = shouldStart ? "start" : "stop";
    setBusy("runtime");
    try {
      const next = await invoke<RuntimeStatus>(
        shouldStart ? "start_runtime" : "stop_runtime",
      );
      applyStatus(next);
      setNotice(
        shouldStart
          ? "采集服务已启动。"
          : "采集服务已停止，服务器连接保持独立运行。",
      );
      runtimeIntentRef.current = null;
    } catch (error) {
      runtimeIntentRef.current = null;
      setNotice(`采集服务操作失败：${String(error)}`);
    } finally {
      runtimeBusyRef.current = false;
      setBusy(null);
    }
  };
  const connect = async (id: string) => {
    if (!serverActionAllowed(id) || !beginConfigChange()) return;
    const previous = config;
    pendingConnectionsRef.current[id] = true;
    serverBusyRef.current[id] = true;
    setBusy(`connect:${id}`);
    setConfig((current) => ({ ...current,
      active_server_id: current.server_mode === "Single" ? id : current.active_server_id,
      server_profiles: current.server_profiles.map(server => server.id === id ? {...server, enabled: true} : server),
    }));
    setStatus((current) => ({ ...current, servers: [
      ...current.servers.filter(server => server.profile_id !== id),
      { profile_id: id, connection: "Connecting", heartbeat_alive: false, last_heartbeat_ms: null, next_retry_at_ms: null, pending: 0, in_flight: 0, blocked: 0 },
    ] }));
    setNotice("正在建立持久连接...");
    try {
      const result = await invoke<RuntimeStatus>("connect_server_profile", {
        id,
      });
      applyStatus(result);
      setNotice("服务器连接已提交，等待认证结果。");
    } catch (error) {
      delete pendingConnectionsRef.current[id];
      setConfig(previous);
      void reload();
      setNotice(`服务器连接失败：${String(error)}`);
    } finally {
      serverBusyRef.current[id] = false;
      setBusy(null);
      endConfigChange();
    }
  };
  const disconnect = async (id: string) => {
    if (!serverActionAllowed(id) || !beginConfigChange()) return;
    const previous = config;
    delete pendingConnectionsRef.current[id];
    serverBusyRef.current[id] = true;
    setConfig((current) => ({ ...current, server_profiles: current.server_profiles.map(server => server.id === id ? {...server, enabled: false} : server) }));
    setBusy(`disconnect:${id}`);
    setNotice("正在断开服务器...");
    try {
      const result = await invoke<RuntimeStatus>("disconnect_server_profile", {
        id,
      });
      applyStatus(result);
      setConfig((current) => ({
        ...current,
        active_server_id:
          current.active_server_id === id ? null : current.active_server_id,
        server_profiles: current.server_profiles.map((server) =>
          server.id === id ? { ...server, enabled: false } : server,
        ),
      }));
      setNotice("服务器已断开。");
    } catch (error) {
      setConfig(previous);
      setNotice(`服务器断开失败：${String(error)}`);
    } finally {
      serverBusyRef.current[id] = false;
      setBusy(null);
      endConfigChange();
    }
  };
  const toggleServer = async (server: Server) => {
    if (server.enabled) await disconnect(server.id);
    else await connect(server.id);
  };
  const scan = async (kind: CollectorKind) => {
    if (scanBusy[kind]) return;
    setScanBusy((current) => ({ ...current, [kind]: true }));
    setScanSummary((current) => ({
      ...current,
      [kind]: { ...current[kind], state: "Scanning" },
    }));
    setNotice(`正在采集${collectorNames[kind]}...`);
    await new Promise<void>((resolve) =>
      requestAnimationFrame(() => resolve()),
    );
    const started = Date.now();
    try {
      const command = persistence?.is_android
        ? "scan_android_environment_now"
        : kind === "wifi"
          ? "scan_wifi_now"
          : "scan_bluetooth_now";
      const args = persistence?.is_android ? { dataType: kind } : undefined;
      const event = await invoke<{ data: unknown }>(command, args);
      setScanData((current) => ({ ...current, [kind]: event.data }));
      setScanSummary((current) => ({
        ...current,
        [kind]: {
          ...current[kind],
          device_count: itemCount(kind, event.data),
          last_scan_ms: Date.now(),
          duration_ms: Date.now() - started,
          successful_scans: (current[kind].successful_scans ?? 0) + 1,
          failed_scans: current[kind].failed_scans ?? 0,
          state: "Ready",
        },
      }));
      setNotice(`${collectorNames[kind]}采集完成。`);
    } catch (error) {
      setScanSummary((current) => ({
        ...current,
        [kind]: {
          ...current[kind],
          failed_scans: (current[kind].failed_scans ?? 0) + 1,
          state: "Error",
        },
      }));
      setNotice(String(error));
    } finally {
      setScanBusy((current) => ({ ...current, [kind]: false }));
    }
  };
  const showDetails = (kind: CollectorKind) => {
    const snapshots: Record<CollectorKind, unknown | null> = {
      wifi: status.wifi_snapshot,
      bluetooth: status.bluetooth_snapshot,
      cell: status.cell_snapshot,
      gps: status.gps_snapshot,
      gnss: status.gnss_snapshot,
    };
    const data = scanData[kind] ?? snapshots[kind];
    if (data)
      setDetails({ kind, title: `${collectorNames[kind]} 采集详情`, data });
    else setNotice("尚未获得采集数据，请先启动采集服务或点击采集。");
  };
  const toggleForeground = async () => {
    if (!persistence) return;
    try {
      setPersistence(
        await invoke<PersistenceSettings>("set_foreground_service_enabled", {
          enabled: !persistence.foreground_enabled,
        }),
      );
      setNotice(
        !persistence.foreground_enabled
          ? "已开启前台常驻服务。"
          : "已关闭前台常驻服务。",
      );
    } catch (error) {
      setNotice(`前台服务设置失败：${String(error)}`);
    }
  };
  const requestBatteryExemption = async () => {
    try {
      await invoke("request_battery_optimization_exemption");
      setNotice("已打开系统电池优化授权页面，请确认允许。");
      window.setTimeout(
        () =>
          invoke<PersistenceSettings>("get_persistence_settings").then(
            setPersistence,
          ),
        1500,
      );
    } catch (error) {
      setNotice(`无法申请电池优化白名单：${String(error)}`);
    }
  };
  const toggleAutoStart = async () => {
    if (!persistence) return;
    try {
      const enabling = !persistence.auto_start_enabled;
      setPersistence(await invoke<PersistenceSettings>("set_auto_start_enabled", { enabled: enabling }));
      setNotice(enabling ? "已启用开机自启动；请同时在系统自启动设置中允许本应用。" : "已关闭开机自启动。" );
    } catch (error) { setNotice(`自启动设置失败：${String(error)}`); }
  };
  const requestAutoStart = async () => {
    try {
      await invoke("request_auto_start_permission");
      setNotice("已打开系统自启动或应用设置，请允许本应用后台自启动。");
    } catch (error) { setNotice(`无法打开自启动设置：${String(error)}`); }
  };
  const toggleHideFromRecents = async () => {
    if (!persistence) return;
    try {
      const enabled = !persistence.hide_from_recents;
      setPersistence(await invoke<PersistenceSettings>("set_hide_from_recents", { enabled }));
      setNotice(enabled ? "已从最近任务列表隐藏；应用和采集服务仍会继续运行。" : "已恢复在最近任务列表中显示。" );
    } catch (error) { setNotice(`多任务显示设置失败：${String(error)}`); }
  };
  const requestAccessibility = async () => {
    try {
      await invoke("request_accessibility_permission");
      setNotice("已打开无障碍设置。请手动启用“采集器后台维护服务”；该服务不读取界面内容或执行点击。" );
      window.setTimeout(() => invoke<PersistenceSettings>("get_persistence_settings").then(setPersistence), 1500);
    } catch (error) { setNotice(`无法打开无障碍设置：${String(error)}`); }
  };
  const requestHomeSettings = async () => {
    try {
      await invoke("request_home_settings");
      setNotice("本应用已注册为启动器候选；仅在专用设备需要时将其设为默认桌面。" );
    } catch (error) { setNotice(`无法打开默认桌面设置：${String(error)}`); }
  };
  const toggleRoot = async () => {
    if (!persistence) return;
    try {
      setPersistence(
        await invoke<PersistenceSettings>("set_root_support_enabled", {
          enabled: !persistence.root_enabled,
        }),
      );
      setNotice(
        !persistence.root_enabled
          ? "Root 增强已启用：OOM 调整为 -1000，并加入 Doze 白名单。"
          : "Root 增强已关闭并尝试恢复系统设置。",
      );
    } catch (error) {
      setNotice(`Root 操作失败或未授权：${String(error)}`);
    }
  };
  const requestDeviceAdmin = async () => {
    try {
      await invoke("request_device_admin");
      setNotice(
        "已打开设备管理员授权页。注意：普通设备管理员不等于 Device Owner，兼容服务仍需按下方命令激活。",
      );
      window.setTimeout(
        () =>
          invoke<PersistenceSettings>("get_persistence_settings").then(
            setPersistence,
          ),
        1500,
      );
    } catch (error) {
      setNotice(`无法打开设备管理员授权页：${String(error)}`);
    }
  };
  const requestBackgroundLocation = async () => {
    try {
      await invoke("request_background_location");
      setNotice(persistence?.android_api_level && persistence.android_api_level >= 30 ? "已打开应用设置，请进入“权限 → 位置信息”并选择“始终允许”。" : "请在系统权限对话框中选择始终允许位置信息。");
      window.setTimeout(() => invoke<PersistenceSettings>("get_persistence_settings").then(setPersistence), 1500);
    } catch (error) { setNotice(`无法申请后台位置权限：${String(error)}`); }
  };
  const toggleDhizukuCompat = async () => {
    if (!persistence) return;
    try {
      setPersistence(
        await invoke<PersistenceSettings>("set_dhizuku_compat_enabled", {
          enabled: !persistence.dhizuku_compat_enabled,
        }),
      );
      setNotice(
        !persistence.dhizuku_compat_enabled
          ? "Dhizuku 兼容服务已开启；支持 Dhizuku API 的应用将发现当前 Owner。"
          : "Dhizuku 兼容服务已关闭。",
      );
    } catch (error) {
      setNotice(`Dhizuku 兼容服务设置失败：${String(error)}`);
    }
  };
  const openDhizukuApps = async () => {
    setDhizukuPage(true);
    setDhizukuBusy("loading");
    try {
      setDhizukuApps(await invoke<DhizukuApp[]>("list_dhizuku_apps"));
    } catch (error) {
      setNotice(`读取 Dhizuku 应用失败：${String(error)}`);
    } finally {
      setDhizukuBusy(null);
    }
  };
  const setDhizukuAuthorization = async (app: DhizukuApp) => {
    if (dhizukuBusy) return;
    setDhizukuBusy(app.package_name);
    try {
      await invoke("set_dhizuku_app_authorization", {
        packageName: app.package_name,
        allowed: !app.allowed,
      });
      setDhizukuApps((current) =>
        current.map((item) =>
          item.package_name === app.package_name
            ? { ...item, allowed: !item.allowed }
            : item,
        ),
      );
    } catch (error) {
      setNotice(`授权变更失败：${String(error)}`);
    } finally {
      setDhizukuBusy(null);
    }
  };

  return (
    <main className="shell">
      <header className="topbar">
        <div>
          <p>远程环境采集器</p>
          <h1>远程环境采集器</h1>
          <span className="device">
            设备 ID: {config.device_id || "正在准备"}
          </span>
        </div>
        <div className={`status ${status.connection === "Ready" ? "ok" : ""}`}>
          ● {stateText(status.connection)}
        </div>
      </header>
      {!persistence?.is_android && (
        <p className="hint">
          自启动托盘模式：给程序快捷方式或启动命令追加参数 <code>--tray</code>
          ，例如 <code>RemoteEnvCollector.exe --tray</code>。以{" "}
          <code>--tray</code>{" "}
          启动时主窗口保持隐藏，程序驻留系统托盘并自动开始采集；需要配置界面时点击托盘菜单「打开主窗口」。
        </p>
      )}
      {notice && (
        <div className="notice" role="status">
          {notice}
          <button aria-label="关闭提示" onClick={() => setNotice(null)}>
            ×
          </button>
        </div>
      )}
      <section className="runtime">
        <div>
          <h2>采集服务</h2>
          <span>
            {status.collection_running ? "运行中" : "已停止"} ·
            服务器连接独立管理 · 待上传 {status.pending} · 发送中{" "}
            {status.in_flight} · 组合包成功 {status.uploaded} · 失败{" "}
            {status.failed}
          </span>
        </div>
        <div className="actions">
          <label>
            上传间隔{" "}
            <input
              type="number"
              min="1"
              max="3600"
              value={config.upload_interval_seconds}
              onChange={(event) =>
                saveOptions({
                  upload_interval_seconds: Number(event.target.value) || 1,
                })
              }
            />{" "}
            秒
          </label>
          <button className="primary" disabled={busy !== null} onClick={toggle}>
            {status.collection_running
              ? busy === "runtime"
                ? "停止中..."
                : "停止采集"
              : busy === "runtime"
                ? "启动中..."
                : "开始采集"}
          </button>
        </div>
      </section>
      {persistence?.is_android && (
        <section className="panel android-persistence">
          <div className="heading">
            <div>
              <h2>Android 后台保活</h2>
              <span>所有增强均由用户主动开启</span>
            </div>
          </div>
          <div className="options">
            <label>
              <input
                type="checkbox"
                checked={persistence.foreground_enabled}
                onChange={toggleForeground}
              />{" "}
              常驻通知与前台服务
            </label>
            <label>
              <input type="checkbox" checked={persistence.auto_start_enabled} onChange={toggleAutoStart} />{" "}
              开机自动启动采集服务
            </label>
            <button onClick={requestAutoStart}>打开系统自启动设置</button>
            <label>
              <input type="checkbox" checked={persistence.hide_from_recents} onChange={toggleHideFromRecents} />{" "}
              从多任务后台隐藏
            </label>
            <button onClick={requestAccessibility} className={persistence.accessibility_enabled ? "ready" : ""}>
              {persistence.accessibility_enabled ? "无障碍维护服务：已启用" : "启用无障碍维护服务"}
            </button>
            <button onClick={requestHomeSettings}>专用设备：设置为默认启动器</button>
            <button
              onClick={requestBatteryExemption}
              disabled={persistence.battery_optimization_ignored}
            >
              {persistence.battery_optimization_ignored
                ? "已在电池优化白名单"
                : "申请电池优化白名单"}
            </button>
            <button onClick={requestBackgroundLocation} disabled={persistence.background_location_granted}>
              {persistence.background_location_granted ? "位置权限：始终允许" : "允许后台位置（始终允许）"}
            </button>
            {!persistence.location_enabled && <span className="permission-warning">系统定位服务未开启，GPS/GNSS 将返回空结果</span>}
            <label title="需要在 KernelSU 或 Magisk 中授权">
              <input
                type="checkbox"
                checked={persistence.root_enabled}
                onChange={toggleRoot}
              />{" "}
              Root 增强（OOM -1000 + Doze 白名单）
            </label>
            <span className={persistence.root_available ? "ready" : "device"}>
              {persistence.root_available ? "已检测到 Root" : "尚未获得 Root"}
            </span>
          </div>
          <details className="owner-card">
            <summary>
              <span>
                <strong>设备所有者与 Dhizuku 兼容</strong>
                <small>
                  {persistence.device_owner_active
                    ? "Device Owner 已激活"
                    : persistence.profile_owner_active
                      ? "Profile Owner 已激活"
                      : "点击展开设置"}
                </small>
              </span>
            </summary>
            <div className="owner-content">
              <button
                onClick={requestDeviceAdmin}
                disabled={persistence.device_admin_active}
              >
                {persistence.device_admin_active
                  ? "设备管理员已启用"
                  : "启用设备管理员"}
              </button>
              <span
                className={
                  persistence.device_owner_active ||
                  persistence.profile_owner_active
                    ? "ready"
                    : "device"
                }
              >
                {persistence.device_owner_active
                  ? "本应用是 Device Owner"
                  : persistence.profile_owner_active
                    ? "本应用是 Profile Owner"
                    : "尚未成为 Device Owner"}
              </span>
              {persistence.dhizuku_supported ? (
                <>
                  <label className="owner-toggle">
                    <input
                      type="checkbox"
                      checked={persistence.dhizuku_compat_enabled}
                      disabled={
                        !persistence.device_owner_active &&
                        !persistence.profile_owner_active
                      }
                      onChange={toggleDhizukuCompat}
                    />{" "}
                    向其他应用提供 Dhizuku API
                  </label>
                  <button
                    onClick={openDhizukuApps}
                    disabled={!persistence.dhizuku_compat_enabled}
                  >
                    应用授权管理
                  </button>
                </>
              ) : (
                <p className="hint">
                  当前 API {persistence.android_api_level} 不在 Dhizuku 2.12.0
                  支持的 Android 8–17 范围内，只能使用设备管理员功能。
                </p>
              )}
              {!persistence.device_owner_active &&
                !persistence.profile_owner_active && (
                  <p className="hint">
                    先取消其他 Device/Profile Owner，并通过 ADB 激活：
                    <code>{persistence.device_owner_command}</code>
                    。普通设备管理员不具备等同能力。
                  </p>
                )}
            </div>
          </details>
        </section>
      )}
      <section className="panel">
        <div className="heading">
          <div>
            <h2>服务器</h2>
            <span>
              {config.server_mode === "Multi" ? "多服务器模式" : "单服务器模式"}
            </span>
          </div>
          <button className="icon" title="新增服务器" onClick={openNew}>
            ＋
          </button>
        </div>
        <div className="options server-modes" aria-busy={configBusy}>
          <label>
            <input
              type="radio"
              name="server-mode"
              disabled={configBusy}
              checked={config.server_mode === "Single"}
              onChange={() => saveOptions({ server_mode: "Single" })}
            />{" "}
            单服务器
          </label>
          <label>
            <input
              type="radio"
              name="server-mode"
              disabled={configBusy}
              checked={config.server_mode === "Multi"}
              onChange={() => saveOptions({ server_mode: "Multi" })}
            />{" "}
            多服务器
          </label>
        </div>
        {config.server_profiles.length === 0 ? (
          <p className="empty">尚未配置服务器。新增后可启动采集服务。</p>
        ) : (
          config.server_profiles.map((server) => {
            const live = status.servers.find(
              (item) => item.profile_id === server.id,
            );
            const connected = live?.connection === "Ready";
            const selected = server.enabled && (config.server_mode === "Multi" || config.active_server_id === server.id);
            const connecting = busy === `connect:${server.id}` ||
              (selected && ["Connecting", "Connected", "Authenticating"].includes(live?.connection ?? ""));
            const retrySeconds = live?.next_retry_at_ms == null ? null :
              Math.max(0, Math.ceil((live.next_retry_at_ms - clock) / 1000));
            const heartbeat = live
              ? heartbeatHealth(live, clock)
              : { className: "heartbeat-off", text: "未连接" };
            return (
              <article className="server" key={server.id}>
                <div className="server-info">
                  <div className="server-title">
                  <strong>{server.name}</strong>
                  {config.server_mode === "Single" ? (
                    <label className="server-select">
                      <input type="radio" name="active-server" disabled={configBusy}
                        checked={config.active_server_id === server.id}
                        onChange={() => saveOptions({ active_server_id: server.id })} />
                      活动服务器
                    </label>
                  ) : (
                    <label className="server-select">
                      <input type="checkbox" checked={server.enabled} disabled={!serverActionAllowed(server.id)}
                        onChange={() => toggleServer(server)} />
                      启用
                    </label>
                  )}
                  </div>
                  <span>{server.url}</span>
                  <small>
                    设备 ID: {server.device_id} ·{" "}
                    {connecting ? "正在连接" : stateText(selected ? live?.connection ?? "Connecting" : "Stopped")} ·{" "}
                    <b className={heartbeat.className}>● {heartbeat.text}</b>
                    {selected && live?.connection === "Reconnecting" && retrySeconds !== null && (
                      <span className="retry-countdown" role="timer">{retrySeconds > 0 ? `${retrySeconds} 秒后重试` : "即将重试…"}</span>
                    )}
                    {live?.last_error && (
                      <em className="connection-error"> · {live.last_error}</em>
                    )}
                  </small>
                </div>
                <div className="actions server-actions">
                      <button
                        className="primary"
                        disabled={!serverActionAllowed(server.id)}
                        title={selected ? "断开并停止重试此服务器" : "连接服务器"}
                        onClick={() =>
                          selected
                            ? disconnect(server.id)
                            : connect(server.id)
                        }
                      >
                        {connecting
                          ? "正在连接…"
                          : busy === `disconnect:${server.id}` ||
                              busy === `toggle:${server.id}`
                            ? "处理中..."
                            : selected
                                ? "断开"
                                : "连接"}
                      </button>
                  <button title="编辑" disabled={configBusy} onClick={() => edit(server)}>
                    编辑
                  </button>
                  <button
                    title="测试连接"
                    disabled={configBusy}
                    onClick={() => testServer(server.id)}
                  >
                    测试
                  </button>
                  <button
                    className="danger"
                    title="删除"
                    disabled={configBusy}
                    onClick={() => removeServer(server.id)}
                  >
                    删除
                  </button>
                </div>
              </article>
            );
          })
        )}
      </section>
      <section className="grid collector-grid">
        <Collector
          title="Wi-Fi"
          subtitle="附近网络"
          countLabel="网络数量"
          state={status.wifi}
          count={
            scanSummary.wifi.device_count ??
            status.wifi_runtime.network_count ??
            itemCount("wifi", scanData.wifi ?? status.wifi_snapshot)
          }
          scan={{ ...status.wifi_runtime, ...scanSummary.wifi }}
          onScan={() => scan("wifi")}
          onDetails={() => showDetails("wifi")}
          busy={scanBusy.wifi}
          now={clock}
        />
        <Collector
          title="蓝牙"
          subtitle="BLE + Classic Bluetooth"
          countLabel="设备数量"
          state={status.bluetooth}
          count={
            scanSummary.bluetooth.device_count ??
            status.bluetooth_runtime.device_count ??
            itemCount(
              "bluetooth",
              scanData.bluetooth ?? status.bluetooth_snapshot,
            )
          }
          scan={{ ...status.bluetooth_runtime, ...scanSummary.bluetooth }}
          onScan={() => scan("bluetooth")}
          onDetails={() => showDetails("bluetooth")}
          busy={scanBusy.bluetooth}
          now={clock}
        />
        {persistence?.is_android && (
          <Collector
            title="基站"
            subtitle="服务小区与邻区"
            countLabel="基站数量"
            state={
              scanSummary.cell.state ??
              (status.collection_running ? "Ready" : "Stopped")
            }
            count={
              scanSummary.cell.device_count ??
              itemCount("cell", scanData.cell ?? status.cell_snapshot)
            }
            scan={{ ...initialStatus.wifi_runtime, ...scanSummary.cell }}
            onScan={() => scan("cell")}
            onDetails={() => showDetails("cell")}
            busy={scanBusy.cell}
            now={clock}
          />
        )}
        {persistence?.is_android && (
          <Collector
            title="GPS"
            subtitle="位置与定位质量"
            countLabel="定位数量"
            state={
              scanSummary.gps.state ??
              (status.collection_running ? "Ready" : "Stopped")
            }
            count={
              scanSummary.gps.device_count ??
              itemCount("gps", scanData.gps ?? status.gps_snapshot)
            }
            scan={{ ...initialStatus.wifi_runtime, ...scanSummary.gps }}
            onScan={() => scan("gps")}
            onDetails={() => showDetails("gps")}
            busy={scanBusy.gps}
            now={clock}
          />
        )}
        {persistence?.is_android && (
          <Collector
            title="GNSS"
            subtitle="卫星与信号状态"
            countLabel="卫星数量"
            state={
              scanSummary.gnss.state ??
              (status.collection_running ? "Ready" : "Stopped")
            }
            count={
              scanSummary.gnss.device_count ??
              itemCount("gnss", scanData.gnss ?? status.gnss_snapshot)
            }
            scan={{ ...initialStatus.wifi_runtime, ...scanSummary.gnss }}
            onScan={() => scan("gnss")}
            onDetails={() => showDetails("gnss")}
            busy={scanBusy.gnss}
            now={clock}
          />
        )}
      </section>
      <section className="panel dbstatus">
        <div className="heading">
          <div>
            <h2>本地数据库</h2>
            <span>配置库与状态缓存分离 · 缓存可安全清理</span>
          </div>
          <button
            className="primary"
            disabled={cleaning}
            onClick={() => cleanupCache()}
          >
            {cleaning ? "清理中..." : "清理状态缓存"}
          </button>
        </div>
        {stateInfo ? (
          <dl className="dbgrid">
            <div>
              <dt>用户配置库</dt>
              <dd>{formatBytes(stateInfo.config_db_bytes)}</dd>
              <small>{stateInfo.config_db_path}</small>
            </div>
            <div>
              <dt>状态缓存库</dt>
              <dd>{formatBytes(stateInfo.state_cache_db_bytes)}</dd>
              <small>{stateInfo.state_cache_db_path}</small>
            </div>
          </dl>
        ) : (
          <p className="empty">正在读取数据库信息…</p>
        )}
        <p className="hint">
          状态缓存库仅用于记录已完成/已取消的上传投递，可随时清理；若文件损坏无法读取，直接删除该文件后程序会自动重建，不影响用户配置库。
        </p>
      </section>
      {dhizukuPage && (
        <div className="page-overlay" role="dialog">
          <section className="settings-page">
            <header className="page-header">
              <button className="back" onClick={() => setDhizukuPage(false)}>‹</button>
              <div><h2>Dhizuku 应用授权</h2><span>仅列出声明 Dhizuku API 权限的应用</span></div>
            </header>
            <div className="app-list">
              {dhizukuBusy === "loading" ? <p className="empty">正在读取应用…</p> : dhizukuApps.length === 0 ? <p className="empty">没有检测到请求 Dhizuku API 的应用。</p> : dhizukuApps.map(app => (
                <article className="app-permission" key={app.package_name}>
                  <div className="app-avatar">{app.label.slice(0, 1).toUpperCase()}</div>
                  <div><strong>{app.label}</strong><span>{app.package_name}</span><small>UID {app.uid}</small></div>
                  <label className="switch"><input type="checkbox" checked={app.allowed} disabled={dhizukuBusy !== null} onChange={() => setDhizukuAuthorization(app)}/><i /></label>
                </article>
              ))}
            </div>
            <p className="permission-note">授权与应用 UID、包名和签名绑定；应用更换签名后会自动失效。取消授权时会同时清除已委派的管理范围。</p>
          </section>
        </div>
      )}
      {dialogOpen && (
        <div className="modal modal-top" role="dialog">
          <div className="dialog server-dialog">
            <div className="heading">
              <h2>{editing ? "编辑服务器" : "新增服务器"}</h2>
              <button
                className="icon"
                onClick={() => {
                  setDialogOpen(false);
                  setEditing(null);
                  setForm({ name: "", url: "", device_id: "", token: "" });
                }}
              >
                ×
              </button>
            </div>
            <label>
              名称
              <input
                value={form.name}
                onChange={(event) =>
                  setForm({ ...form, name: event.target.value })
                }
              />
            </label>
            <label>
              WebSocket 地址
              <input
                placeholder="wss://example.com/envser/ws"
                value={form.url}
                onChange={(event) =>
                  setForm({ ...form, url: event.target.value })
                }
              />
            </label>
            <label>
              设备 ID
              <input
                placeholder="服务器端已注册的 Device ID"
                value={form.device_id}
                onChange={(event) =>
                  setForm({ ...form, device_id: event.target.value })
                }
              />
            </label>
            <label>
              令牌{editing && <small>留空则保留已有令牌</small>}
              <div className="token">
                <input
                  type={showToken ? "text" : "password"}
                  value={form.token}
                  onChange={(event) =>
                    setForm({ ...form, token: event.target.value })
                  }
                />
                <button type="button" onClick={() => setShowToken(!showToken)}>
                  {showToken ? "隐藏" : "显示"}
                </button>
              </div>
            </label>
            <div className="dialog-actions">
              <button
                onClick={() => {
                  setDialogOpen(false);
                  setEditing(null);
                  setForm({ name: "", url: "", device_id: "", token: "" });
                }}
              >
                取消
              </button>
              <button className="primary" onClick={saveServer}>
                保存
              </button>
            </div>
          </div>
        </div>
      )}
      {details && (
        <div className="modal" role="dialog" onClick={() => setDetails(null)}>
          <div
            className="dialog detail-dialog"
            onClick={(event) => event.stopPropagation()}
          >
            <div className="heading">
              <h2>{details.title}</h2>
              <button className="icon" onClick={() => setDetails(null)}>
                ×
              </button>
            </div>
            <div className="detail-list">
              {detailItems(details.kind, details.data).map((item, index) => (
                <div className="detail-row" key={`${item.label}-${index}`}>
                  <span>{item.label}</span>
                  <strong>{item.value}</strong>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}
    </main>
  );
}
function Collector({
  title,
  subtitle,
  countLabel,
  state,
  count,
  scan,
  onScan,
  onDetails,
  busy,
  now,
}: {
  title: string;
  subtitle: string;
  countLabel: string;
  state: string;
  count: number | null;
  scan: Scan;
  onScan: () => void;
  onDetails: () => void;
  busy: boolean;
  now: number;
}) {
  const active =
    state === "Ready" || state === "Scanning" || state === "Running";
  return (
    <section className="panel collector">
      <div className="heading">
        <div>
          <h2>{title}</h2>
          <span>{subtitle} · 全量采集</span>
        </div>
        <span className={`always-on ${active ? "" : "inactive"}`}>
          {active ? "持续采集中" : "等待采集"}
        </span>
      </div>
      <strong className={state === "Ready" ? "ready" : ""}>
        {stateText(state)}
      </strong>
      <dl>
        <div>
          <dt>{countLabel}</dt>
          <dd>{count ?? "-"}</dd>
        </div>
        <div>
          <dt>上次扫描</dt>
          <dd>{ago(scan.last_scan_ms, now)}</dd>
        </div>
        <div>
          <dt>采集耗时</dt>
          <dd>
            {scan.duration_ms
              ? `${(scan.duration_ms / 1000).toFixed(1)} 秒`
              : "-"}
          </dd>
        </div>
        <div>
          <dt>成功 / 失败</dt>
          <dd>
            {scan.successful_scans} / {scan.failed_scans}
          </dd>
        </div>
      </dl>
      <div className="collector-actions">
        <button className="primary" disabled={busy} onClick={onScan}>
          {busy ? "采集中..." : "采集"}
        </button>
        <button onClick={onDetails}>详情</button>
      </div>
      {scan.last_error && (
        <p className="error">
          {title} 采集失败，将按指数退避自动重试（最长 120 秒）。
        </p>
      )}
    </section>
  );
}
createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
