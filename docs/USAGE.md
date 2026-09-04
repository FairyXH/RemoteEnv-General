# RemoteEnvCollector 使用手册

## 1. 准备服务器信息

在 RemoteEnvServer 创建设备并取得完整 WebSocket 地址、设备 ID 和 collector Token。地址示例为 `ws://192.168.1.10:8000/ws` 或 `wss://example.com/envser/ws`。设备 ID 与 Token 必须属于同一服务端设备，反向代理部署在子路径时 URL 必须包含该前缀。

## 2. Windows 安装与运行

安装版运行 `RemoteEnvCollector-Setup.exe`；Portable 版保留完整目录并运行 `RemoteEnvCollector.exe`。程序不需要 Node.js、Rust 或 Python。

关闭主窗口后程序缩到托盘，采集仍继续；必须使用托盘“退出”才能完全停止 Runtime。配置、SQLite 队列和日志通常位于 `%APPDATA%\com.remoteenv.collector\`，日志为 `logs\collector.log`。

## 3. Android 安装与运行

安装已签名的 `RemoteEnvCollector-Android-v0.2.0.apk`，首次打开后授予所需附近设备、位置、电话状态和通知权限。后台位置通常要在系统应用权限页单独选择“始终允许”。

无人值守场景应启用前台服务、开机自动启动、忽略电池优化以及厂商系统的自启动/后台运行许可。Root 设备可选装 `RemoteEnvCollector-Magisk-KernelSU.zip`，但应先在 APK 内完成服务器配置并验证连接，详见 [Android 文档](ANDROID.md)。

## 4. 配置服务器

新增或编辑服务器时填写：名称、完整 `ws://`/`wss://` URL、设备 ID、Token，并启用该配置。

单服务器模式只向活动服务器发送；多服务器模式向所有启用服务器创建独立投递。同一事件对所有目标使用相同序列和内容，但连接、重试、ACK 与错误状态彼此隔离。

保存后先执行连接测试。测试成功只证明当前参数可握手，持续运行还要观察心跳、采集与 ACK。

## 5. 启动与验收

启动采集后确认：

- 服务器从“正在连接”变为“已连接”；
- 最近心跳时间持续刷新；
- Wi-Fi、蓝牙等最近采集时间持续更新；
- 成功计数增长；
- 待投递/处理中队列不会无限增长。

常驻通知、托盘图标或进程存在都只代表外壳或服务存活，不能单独证明 WebSocket 和采集链路已经工作。

## 6. 可靠投递

每种数据类型使用独立的 `environment_data` 事件。Core 在 SQLite 中保存序列与服务器目标状态：断网时保留事件并重连；重启时恢复未完成投递；单个服务器鉴权失败不影响其他服务器；收到匹配 ACK 后完成对应投递。

服务端可能限制单位时间上传数量。采集间隔过短会触发 `rate_limited` 并产生积压。

## 7. 故障排查

### 一直“正在连接”

检查 URL 协议、端口、`/ws` 路径和代理前缀；Token/设备 ID 绑定；客户端 DNS、路由、防火墙；`wss://` 证书链、域名和系统时间；服务端 collector WebSocket 是否运行。

### 已连接但无数据

检查采集开关、权限/硬件、最近采集时间和待 ACK 队列。Android 需确认系统位置总开关，Windows 需确认 WLAN/Bluetooth 服务和适配器。

### 队列持续增加

通常表示服务端没有返回匹配 `data_result`、触发限流、网络反复断开，或多服务器中的某个目标不可用。应逐个检查服务器状态。

### Android 有通知但无连接/采集

```powershell
adb logcat -s RemoteEnvCollector AndroidRuntime
adb shell dumpsys activity services com.remoteenv.collector
```

重点搜索 `Starting headless runtime`、`Headless runtime start failed`、`Headless collection failed` 和连接错误。

## 8. 升级与卸载

Windows 升级前退出旧版本，再运行新安装器或替换 Portable 目录。Android 更新包必须使用与已安装版本相同的证书；覆盖安装可保留配置，卸载会删除应用私有数据。Root 模块与 APK 相互独立，不会随对方自动卸载。

## 9. 提交故障信息

请提供应用版本、平台/系统版本、发生时间、服务器状态、采集状态、队列计数及相关日志片段。提交前删除 Token、Cookie、Authorization header、精确位置、SSID/BSSID 和蓝牙地址等敏感信息。
