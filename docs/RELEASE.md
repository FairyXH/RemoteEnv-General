# Windows Release

## 输出目录

唯一正式发版目录是：

```text
Release/Windows/
```

Portable 包位于 `Release/Windows/RemoteEnvCollector/`，至少包含 `RemoteEnvCollector.exe`。如果安装器工具可用，脚本还会生成 `Release/Windows/RemoteEnvCollector-Setup.exe`。

## 构建

在 Windows PowerShell 中，从仓库根目录执行：

```powershell
./scripts/build-release.ps1
```

脚本会清理旧的 `Release/Windows/`，构建 React production bundle，再用 Tauri CLI 构建包含前端资源的 Windows executable 和 NSIS installer，并做 artifact 存在性检查。若未安装 `app/ui/node_modules`，请先执行 `npm ci`。

版本统一维护在 workspace `Cargo.toml`、`app/ui/package.json` 和 `app/desktop/src-tauri/tauri.conf.json`，当前版本为 `0.2.0`。Tauri 的 Windows 文件描述、图标和应用名来自桌面配置；图标文件为 `app/desktop/src-tauri/icons/icon.ico`。

## 使用与升级

Portable：复制整个 `Release/Windows/RemoteEnvCollector/` 目录到普通 Windows 机器后直接运行 `RemoteEnvCollector.exe`。不要只复制 exe 外的开发目录，也不需要 Node.js、Rust 或 Python。

Installer：运行 `RemoteEnvCollector-Setup.exe`，按安装向导完成安装。卸载只移除应用文件，不应手动删除用户数据。升级前退出应用，再替换 Portable 目录或运行新版安装器；用户配置和队列位于应用数据目录，不随二进制目录迁移。

## 用户数据

Tauri 使用 Windows 应用数据目录保存 SQLite 状态、身份、服务器配置和运行队列，路径由 `app_data_dir` 决定，通常为：

```text
%APPDATA%\com.remoteenv.collector\
```

Release 不向 `Program Files`、项目源码目录或 exe 所在目录写入运行数据。令牌只保存在 Windows 当前用户可解密的 DPAPI 保护内容中和连接请求内；旧版本明文配置可兼容读取，并会在下次配置保存时转换为 DPAPI 保护格式。UI 默认隐藏令牌，状态快照、日志和错误提示不返回令牌。

Windows Release 启动时使用命名单实例 Mutex `Global\\RemoteEnvCollector.SingleInstance`。重复启动不会创建第二个 Runtime 或争用同一 SQLite 状态，而是返回已在运行的启动错误。

## 运行行为

主窗口关闭会隐藏到系统托盘，采集服务继续运行。托盘提供打开、启动采集服务、停止采集服务和退出。只有托盘的“退出”会停止采集服务、采集器、服务器 worker 并退出进程。

状态正常通过 `runtime_status_changed` 推送到 UI；首次加载或事件恢复时使用 `get_runtime_status` 快照。Wi-Fi 与 BLE + Classic Bluetooth 共用 Runtime、事件、SQLite delivery 和多服务器上传链路。

## 故障日志

当前版本把结构化运行状态显示在主界面，包括服务器连接、队列计数、扫描耗时、成功/失败计数和可读的采集错误。采集错误不会打印认证令牌、Cookie 或 Authorization header。运行日志写入 `%APPDATA%\com.remoteenv.collector\logs\collector.log`，最多保留最近 5 MB，超过后轮转为 `collector.log.1`；日志至少记录启动、服务器连接请求、扫描成功/失败和采集服务关键错误。收集故障信息时请提供应用版本、Windows 版本、主界面状态和复现时间；不要发送 `state.sqlite3` 或包含令牌的配置内容。

## 验收命令

```powershell
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace --no-fail-fast
Set-Location app/ui
npm run build
Set-Location ../..
./scripts/build-release.ps1
```

最后只验证 `Release/Windows/` 中的 artifact。当前环境已完成源码、UI、Tauri 打包和进程级启动/停止验证；无可视桌面自动化通道，不能将其表述为安装器窗口、Tray 点击、真实硬件按钮或 clean-machine 验证。

UI 冒烟检查应覆盖：新增/编辑服务器、连接、测试、启动/停止、Wi-Fi 扫描、蓝牙扫描、详情弹窗、重复点击锁定、状态和心跳实时刷新。
