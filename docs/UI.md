# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The UI reads `get_runtime_status` and renders the real `servers` list, including each profile's connection state and target delivery counts. Wi-Fi and Bluetooth status are sourced from the Runtime status model, including continuously pushed latest snapshots. Bluetooth is represented by one shared Runtime status card; detailed payloads use the server-compatible `devices` list and retain `mode` (`ble`, `classic`, `dual`) plus raw advertisement sections. Card counts and detail dialogs update from background scans without requiring the manual Scan action. When both collectors are enabled, the Runtime displays one combined Bluetooth envelope upload success/failure count per ACK/result.

## Phase 2-C desktop UI

主界面现在提供采集服务启动/停止、单/多服务器模式、服务器新增/编辑/删除/启用、持久连接、连接测试、令牌显示/隐藏、Wi-Fi 与统一 Bluetooth 开关、即时扫描、扫描详情、扫描间隔和队列统计。令牌不在状态模型中返回；编辑已有服务器时令牌留空表示保留原值。UI 和用户可见状态均为简体中文。

状态首次加载使用 `get_runtime_status`，之后由 Tauri 的 `runtime_status_changed` 事件更新；快照仍可用于恢复和调试。

## 服务器测试与持久连接

“测试”只验证 WebSocket、认证和 `device_list`，认证成功后立即返回，不等待服务端额外心跳，因此不会因为测试连接被服务端主动关闭而误报超时。“连接”会把指定 Profile 设为活动服务器并启动/更新 Runtime，由正式 ServerWorker 保持 WebSocket、心跳和重连。

Wi-Fi 和蓝牙卡片的“扫描”按钮执行一次原生 Windows 扫描；“详情”显示最近一次扫描的完整 JSON，蓝牙详情保留 BLE/Classic 来源和 RAW 广告字段。


Tauri desktop owns the tray boundary。关闭主窗口会隐藏到托盘，采集服务不会因此停止。托盘提供打开、启动采集服务、停止采集服务和退出；退出路径先停止采集服务、采集器和 ServerWorker，再结束进程。

Phase 2 UI status rules: Start is the collection activation command and also creates missing selected ServerWorkers; Connect works independently and shows transitional backend status. Stop removes and joins the Runtime, so server heartbeat/reconnect and collector work end. Server cards show heartbeat health from the last successful pong: green at <=5 seconds, yellow above 5 seconds, red above 30 seconds and after the 60-second reconnect threshold. Release process smoke passed, but pywinauto WebView2 UIA enumeration timed out; real packaged click/input/screenshot acceptance remains pending.

Windows scan detail data is now rendered from server-compatible payload keys (`rssi`, canonical Wi-Fi `band`/`security`, Bluetooth `technology`, `service_uuids`, Base64 byte fields). Native-only diagnostic metadata is not part of the uploaded standard payload.
