# RemoteEnvCollector

RemoteEnvCollector 是 RemoteEnvServer 的跨平台环境数据采集端。它持续采集设备附近的无线环境信息，通过 WebSocket 可靠上传，并在本地持久化配置、序列号和待确认投递。

当前版本为 **0.2.0**，提供 Windows 桌面端和 Android 端。Android 的真正运行主体是前台 `Service`：服务进程直接启动 Rust Runtime、连接服务器并执行采集，Tauri Activity 只承担配置界面，不参与后台保活，因此由开机广播或 Magisk/KernelSU 模块拉起服务时不会闪现应用窗口。

## 功能

- Windows：持续采集 Wi-Fi、BLE 与经典蓝牙。
- Android：持续采集 Wi-Fi、BLE/经典蓝牙、蜂窝网络、GPS 与 GNSS。
- 单服务器和多服务器投递；每个服务器拥有独立连接、心跳、重连和 ACK 状态。
- SQLite 持久化全局递增序列与目标级投递队列，断网、重启后继续发送。
- WebSocket `ws://` / `wss://`，Token 鉴权，指数退避重连。
- Windows 托盘常驻；关闭窗口不停止采集。
- Android 前台服务、开机自启、电池优化引导、Root 加强、Device Owner 和 Dhizuku 兼容。
- Magisk/KernelSU 模块可在 Root 层启动并守护服务、授予运行权限、加入 Doze 白名单、降低 OOM 优先级并维护无障碍服务状态。

## 下载

Release 中的主要文件：

| 文件 | 用途 |
| --- | --- |
| `RemoteEnvCollector-Setup.exe` | Windows 安装版 |
| `RemoteEnvCollector.exe` | Windows Portable 主程序 |
| `RemoteEnvCollector-Android-v0.2.0.apk` | 已签名 Android APK |
| `RemoteEnvCollector-Magisk-KernelSU.zip` | 可选 Root 守护模块 |

完整安装、配置、运行和排障步骤见 [使用文档](docs/USAGE.md)。Android 的实现与 Root 模式说明见 [Android 文档](docs/ANDROID.md)。

## 快速开始

1. 在 RemoteEnvServer 中创建设备，取得 WebSocket 地址、设备 ID 和 Token。
2. 安装对应平台客户端并打开配置界面。
3. 新增服务器，填写名称、地址、设备 ID 与 Token，保存并测试连接。
4. 启动采集，确认服务器状态为“已连接”，队列可以被 ACK 后清空。
5. Android 若要求无人值守运行，先启用前台服务和开机自启；Root 设备可再刷入守护模块。

WebSocket 地址通常为 `ws://host:port/ws` 或 `wss://host/prefix/ws`。设备 ID 必须与 Token 在服务端绑定的设备一致。

## 架构

```text
React 配置 UI
       |
Tauri commands / events
       |
Rust Core: 配置、SQLite 队列、序列、调度、WebSocket
       |
平台适配器 / Android 原生 Service
       |
Wi-Fi / Bluetooth / Cell / GPS / GNSS
```

采集器不会直接操作 WebSocket。平台层产生规范化 `CollectorEvent`，Core 为事件分配持久序列，为每个目标服务器创建独立投递，并等待与 `(device_id, data_type, sequence)` 匹配的 ACK。一个目标断线或鉴权失败不会阻塞其他目标。

目录说明：

- `crates/core`：配置、协议、队列、Runtime 和服务器 Worker。
- `crates/platform-*`：各平台采集适配器。
- `app/ui`：共享 React + TypeScript 配置界面。
- `app/desktop/src-tauri`：Tauri 桌面/移动壳及生命周期集成。
- `app/desktop/src-tauri/gen/android`：Android 原生工程、前台服务与采集桥。
- `android/root-module`：Magisk/KernelSU 守护模块源码。
- `scripts`：各平台构建脚本。
- `docs`：协议、架构、开发和使用文档。

## 构建

基础环境：Rust 1.85+、Node.js 22+、npm。Windows 构建还需要 MSVC Build Tools 与 WebView2；Android 构建需要 JDK 17、Android SDK/NDK 和 Rust Android targets。

```powershell
npm --prefix app/ui ci
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast
npm --prefix app/ui run build

pwsh -File scripts/build-windows.ps1
pwsh -File scripts/build-android.ps1 -ReleaseApk
pwsh -File scripts/build-root-module.ps1
```

一键构建当前主机支持的平台：

```powershell
pwsh -File scripts/build-all-platforms.ps1 -ReleaseAndroid
```

输出统一位于 `Release/<Platform>/`。桌面应用只能在对应原生系统上打包。构建脚本的参数和行为见 [scripts/README.md](scripts/README.md)，开发环境详见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 协议与数据安全

- 上传帧是 UTF-8 JSON，事件类型为 `environment_data`。
- 序列号按 `(device_id, data_type)` 持久化并严格递增。
- Token 不写入运行日志或状态快照；Windows 配置使用当前用户 DPAPI 保护。
- Android 的应用私有数据、SQLite 队列和服务配置位于系统应用数据目录。
- 日志与诊断资料可能包含设备和无线环境元数据，提交问题前应自行检查。

协议细节见 [docs/PROTOCOL.md](docs/PROTOCOL.md)，设计细节见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

## 平台状态

| 平台 | 状态 | 说明 |
| --- | --- | --- |
| Windows | 可用 | Wi-Fi、BLE、经典蓝牙、托盘后台运行 |
| Android | 可用 | API 26+，独立前台 Service；权限与硬件能力决定可采集项 |
| Linux | 适配层占位 | 尚未提供正式采集实现 |
| macOS | 适配层占位 | 尚未提供正式采集实现 |

## 更多文档

- [使用手册](docs/USAGE.md)
- [Android 与 Root 模式](docs/ANDROID.md)
- [开发指南](docs/DEVELOPMENT.md)
- [构建发布](docs/RELEASE.md)
- [WebSocket 协议](docs/PROTOCOL.md)
- [系统架构](docs/ARCHITECTURE.md)
- [变更记录](docs/CHANGELOG.md)

## License

仓库当前未声明开源许可证。除非版权所有者另行授权，不应假定获得复制、修改或再分发权利。
