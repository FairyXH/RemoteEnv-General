# UI

## Shared UI

`app/ui` is the only product UI. Tauri renders it on Windows and is planned to render it on Android. Responsive layout adapts the same views; there are no per-platform product screens.

## First Windows surface

The UI reads `get_runtime_status` and renders the real `servers` list, including each profile's connection state and target delivery counts. Wi-Fi and Bluetooth status are sourced from the Runtime status model. Bluetooth is now represented by one shared Runtime status card; the UI does not claim separate BLE/Classic upload states. Detailed payloads retain `transport` (`ble`, `classic`, `dual`) and raw AD sections, while the main status displays the aggregate Bluetooth worker.

## Phase 2-C desktop UI

主界面现在提供运行时启动/停止、单/多服务器模式、服务器新增/编辑/删除/启用、连接测试、令牌显示/隐藏、Wi-Fi 与统一 Bluetooth 开关、扫描间隔和队列统计。令牌不在状态模型中返回；编辑已有服务器时令牌留空表示保留原值。UI 和用户可见状态均为简体中文。

状态首次加载使用 `get_runtime_status`，之后由 Tauri 的 `runtime_status_changed` 事件更新；快照仍可用于恢复和调试。

## Tray

Tauri desktop owns the tray boundary。关闭主窗口会隐藏到托盘，Runtime 不会因此停止。托盘提供打开、启动运行时、停止运行时和退出；退出路径先停止 Runtime、采集器和 ServerWorker，再结束进程。
