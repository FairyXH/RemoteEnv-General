# Android 实现与部署

## 运行模型

Android 端由两个彼此解耦的部分组成：

- `MainActivity` / Tauri WebView：只用于编辑配置、查看状态和执行交互操作。
- `CollectorForegroundService`：后台本体，直接初始化 Rust `HeadlessRuntime`、建立 WebSocket、周期采集并提交事件。

从开机广播、系统重启恢复或 Magisk/KernelSU 模块启动时，只启动前台 Service，不启动 Activity，因此不会显示或闪现应用窗口。Service 使用 `START_STICKY`，并显示 Android 要求的低重要性常驻通知。用户点击通知时才打开配置界面。

Service 与 UI 使用相同的应用数据目录。UI 保存的服务器、设备 ID、Token、采集开关和间隔可由 Headless Runtime 直接读取；服务停止时会停止原生采集循环和 Rust Runtime。

## 兼容性与数据

- 最低版本 Android 8.0 / API 26，compile/target SDK API 36。
- 默认 Release 为 `arm64-v8a`，也可构建 universal APK。
- 网络层使用 Rustls，不依赖 Android OpenSSL。
- 采集 `wifi`、`bluetooth`、`cell`、`gps` 和 `gnss` 独立事件。

无权限、无硬件或某项 API 失败只影响对应数据类型，不会终止整个采集周期。Android 厂商会限制后台扫描频率，系统也可能返回缓存的 Wi-Fi/BLE 结果。

## 权限与后台运行

按实际采集需求授予精确/粗略位置、后台位置、附近设备、蓝牙、附近 Wi-Fi、电话状态和通知权限。Android 通常要求先授予前台位置，再单独授予后台位置。系统位置总开关关闭也会限制 Wi-Fi、BLE、GPS/GNSS。

无人值守运行建议依次启用前台服务、开机自启、忽略电池优化和厂商系统的自启动/后台许可。看到常驻通知只说明 Service 存活；真正运行还应确认服务器已连接、最近采集时间持续更新、队列能在 ACK 后下降。

## Magisk/KernelSU 守护模块

`RemoteEnvCollector-Magisk-KernelSU.zip` 是可选 Root 模块。守护脚本每 30 秒：

- 检查 APK 并直接启动 `.nativecollector.CollectorForegroundService`，不启动界面；
- 授予当前 Android 版本支持的运行权限；
- 允许后台运行并加入 Doze 白名单；
- 对进程应用 `oom_score_adj=-1000`；
- 在 Root 层合并并维护无障碍服务列表，不覆盖其他服务。

先安装并配置 APK，确认至少一次正常连接，再从 Magisk 或 KernelSU 安装模块并重启。模块不包含 APK。临时停用可在模块目录创建 `disable` 文件，或直接在管理器中禁用模块。

## Device Owner 与 Dhizuku

设备管理员不等于 Device/Profile Owner。完整 Owner 模式需要在满足 Android 配置条件的设备上使用界面给出的 `adb shell dpm set-device-owner ...` 命令。

成为 Device Owner 或 Profile Owner 后，可显式启用 Dhizuku Server 兼容 Provider `com.remoteenv.collector.dhizuku_server.provider`。该入口默认关闭；应用不再是 Owner 或关闭开关时不会返回 Binder。

## ADB 排障

```powershell
adb shell dumpsys activity services com.remoteenv.collector
adb shell pidof com.remoteenv.collector
adb shell dumpsys package com.remoteenv.collector
adb logcat -s RemoteEnvCollector AndroidRuntime
```

- 有通知但一直连接：检查 URL `/ws` 路径、Token/设备 ID、DNS/网络和 TLS 证书。
- 服务运行但无采集：检查权限、系统位置开关及 `Headless collection failed`。
- 重启后未运行：确认开机自启；Root 模式检查模块是否启用。
- 更新安装失败：相同包名必须使用同一签名证书。

## 构建与签名

```powershell
pwsh -File scripts/build-android.ps1 -ReleaseApk
pwsh -File scripts/build-android.ps1 -ReleaseApk -MultiAbi
pwsh -File scripts/build-root-module.ps1
```

Release APK 必须先使用 Android SDK 的 `zipalign` 和 `apksigner` 签名，并执行 `apksigner verify --verbose --print-certs`。密钥库和口令只能保存在构建机，不应加入仓库或 Release。
