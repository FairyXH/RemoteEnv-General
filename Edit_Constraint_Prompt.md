# RemoteEnvCollector 项目通用修改要求

你正在修改项目：

`D:\Files\Develop\Cross-Platform\RemoteEnvCollector`

请先阅读项目现有文档，尤其是：

* `docs/CONTEXT.md`
* `docs/ARCHITECTURE.md`
* `docs/DEVELOPMENT.md`
* `docs/PROTOCOL.md`
* `docs/WEBSOCKET.md`
* `docs/UI.md`
* `docs/PLATFORM.md`
* 与当前任务直接相关的其他 `docs/*.md`

**开始修改前必须先阅读 `docs/CONTEXT.md`，并根据其中记录的当前架构、约束、已知问题和后续计划继续工作。不要假设前一个 Agent 的实现状态；以项目代码和会话文档中的实际内容为准。**

---

## 一、项目总体要求

这是一个跨平台的 Remote Environment Collector 客户端，用于采集本机环境信息并通过 WebSocket 上传到 RemoteEnvServer。

项目需要保持清晰的跨平台架构：

* `crates/core`：平台无关核心逻辑
* `crates/platform-windows`：Windows 平台实现
* `crates/platform-linux`：Linux 平台实现
* `crates/platform-macos`：macOS 平台实现
* `crates/platform-android`：Android 平台实现
* `app/desktop/src-tauri`：桌面端 Tauri
* `app/ui`：统一 React + TypeScript UI

平台相关 API 必须放在对应平台 crate 中，Core 不应直接依赖具体平台 API。

---

## 二、Server API 必须严格遵循服务端文档

服务端项目及 API 文档位于：

`D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer`

API 文档：

`D:\Files\Develop\Algorithm_Development\Python\RemoteEnvProject\RemoteEnvServer\docs\api.md`

**任何涉及服务端通信、HTTP API、WebSocket、认证、设备信息、数据上传、ACK、心跳、配置等修改，都必须先阅读该 API 文档。**

客户端实现必须与服务端 API 文档严格一致，包括但不限于：

* URL
* HTTP Method
* WebSocket endpoint
* 请求参数
* 参数名称
* 参数类型
* JSON 结构
* 字段名称
* 字段大小写
* 数据类型
* 必填/可选字段
* 枚举值
* WebSocket message type
* auth 格式
* heartbeat 格式
* environment_data 格式
* data_result / ACK 格式
* 错误格式

**不能根据猜测自行设计协议。**

如果客户端现有实现与 `docs/api.md` 不一致，应以服务端 API 文档为准进行修正。

---

## 三、Device ID 与多服务器

客户端支持多个 ServerProfile / 多服务器同时运行。

**不同服务器提供的 Device ID 不保证相同。**

Device ID 必须以用户在对应 ServerProfile 中输入/配置的 Device ID 为准。

也就是说：

```text
Server A
Device ID = 用户为 A 配置的 Device ID

Server B
Device ID = 用户为 B 配置的 Device ID
```

不能假设：

```text
所有服务器共享同一个 device_id
```

每个服务器的认证凭据必须独立保存和使用：

* Server URL
* Device ID
* Token
* enabled
* 其他服务器配置

Worker 在连接某个服务器时，必须使用该 ServerProfile 自己的 Device ID 和凭据。

---

## 四、Sequence 要求

所有 Event 的 sequence 必须采用：

**毫秒级 Unix 时间戳。**

即：

```text
SystemTime / UNIX_EPOCH
```

转换为：

```text
milliseconds
```

sequence 不使用：

* 自增整数
* 随机数
* UUID
* 纳秒时间戳
* 秒级时间戳

需要注意服务端对 sequence 的实际协议约束，并确保客户端生成的 sequence 满足服务端要求。

多服务器上传时，各服务器可以针对自己的 Device ID / ServerProfile 使用符合协议要求的 sequence 语义，但不得破坏同一 Event 的数据一致性。

---

## 五、WebSocket Worker 要求

每一个**已启用服务器**都必须拥有独立的 WebSocket Worker。

例如：

```text
Server A
    └── Worker A
         └── WebSocket A

Server B
    └── Worker B
         └── WebSocket B

Server C
    └── Worker C
         └── WebSocket C
```

服务器之间必须相互隔离：

* 独立连接
* 独立认证
* 独立 Device ID
* 独立 Token
* 独立 heartbeat
* 独立 reconnect
* 独立 in-flight delivery
* 独立 ACK
* 独立错误状态
* 独立连接生命周期

某服务器断线时：

**不能影响其他服务器继续工作。**

---

## 六、Heartbeat

WebSocket heartbeat 必须：

**每 5 秒发送一次。**

即：

```text
heartbeat interval = 5 seconds
```

必须按照服务端 API / WebSocket 文档规定的 heartbeat 格式实现。

需要正确处理：

```text
heartbeat
    ↓
server pong
    ↓
连接保持 Ready
```

如果服务端要求 pong 超时检测，也必须按照协议实现。

---

## 七、Reconnect

所有 `enabled = true` 的 ServerProfile：

**WebSocket 必须无限重试。**

网络异常、连接断开等可恢复错误不能让服务器永久停止。

采用指数退避，例如：

```text
1s
2s
4s
8s
16s
30s
30s
30s
...
```

具体上限按照项目现有设计或服务端实际要求确定。

要求：

* 无限重试
* 指数退避
* 有最大退避上限
* Runtime stop 时必须立即停止等待
* Worker 被禁用时停止
* ServerProfile 被删除时停止
* URL / Token / Device ID 修改后停止旧 Worker，并使用新配置启动新 Worker

**认证永久失败等不可恢复状态可以进入 Blocked，但必须依据服务端协议判断，不能简单把所有连接失败都视为永久失败。**

---

## 八、上传架构

采集器不能直接操作 WebSocket。

统一使用：

```text
Collector
    ↓
CollectorEvent
    ↓
Sequence
    ↓
upload_deliveries
    ↓
UploadDispatcher
    ↓
ServerWorker
    ↓
WebSocket
    ↓
Server
    ↓
ACK
```

Collector 必须保持平台采集与上传逻辑解耦。

Collector 不应该直接：

* 创建 WebSocket
* 发送 WebSocket message
* 操作 ServerWorker
* 操作 upload_deliveries
* 处理服务器 ACK

---

## 九、统一数据模型

不同平台实现相同能力时，优先使用 Core 中的平台无关数据结构。

例如：

```text
WiFi
Bluetooth
BLE
Classic Bluetooth
Base Station
```

应该尽可能统一为对应的 `CollectorEvent` / snapshot / observation 数据模型。

不要为了某一个平台重新设计一套上传协议。

所有 Collector 最终都必须通过统一：

```text
CollectorEvent
```

进入 Runtime 上传链路。

---

## 十、UI

UI 使用：

**统一 React + TypeScript UI。**

不要为 Windows / Linux / macOS 分别制作完全独立的 UI。

平台差异应该通过：

```text
RuntimeStatus
Platform capabilities
Tauri commands
```

体现。

UI 必须具备完整、可实际使用的桌面应用体验，而不是只提供开发测试页面。

涉及功能时应同步考虑：

* 状态
* 错误提示
* Loading
* Disabled 状态
* 配置修改
* Runtime 状态
* 多服务器状态
* Collector 状态
* 数据统计

---

## 十一、Release 发布目录

项目正式发版目录固定为：

`D:\Files\Develop\Cross-Platform\RemoteEnvCollector\Release`

**所有最终可发布构建产物必须进入 `Release`。**

按平台建立子目录，例如：

```text
Release/
├── Windows/
├── Linux/
├── macOS/
├── Android/
└── ...
```

具体平台使用自己的子目录。

不要把最终发布文件只留在：

```text
target/debug/
target/release/
```

等 Cargo 默认构建目录。

这些目录可以作为中间构建目录，但最终发版文件必须复制/构建到：

```text
Release\<Platform>\
```

---

## 十二、最终可执行文件标准

Release 中的最终程序必须满足：

**在没有 Rust、Node.js、Cargo、开发工具等开发环境的普通机器上能够独立运行。**

最终发布版本必须：

* 包含运行所需的程序文件
* 不依赖开发环境
* 不要求用户安装 Rust
* 不要求用户安装 Node.js
* 不要求用户运行 `cargo`
* 不要求用户启动 Vite development server
* 不依赖项目源码
* 不依赖开发服务器
* 不使用仅开发环境可用的路径
* 配置、资源、运行时依赖必须正确打包

同时必须具备：

* 完整 UI
* 正常启动
* 正常退出
* 配置保存
* Runtime 启停
* ServerProfile 管理
* Collector 状态显示
* 错误状态显示
* 多服务器状态显示
* 正常网络异常恢复

**Release 构建必须作为正式验收的一部分，而不是仅验证 `cargo check`。**

---

## 十三、构建与验证

修改完成后至少执行与当前任务相关的验证。

通常包括：

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

如果涉及 UI：

```text
Set-Location app/ui
npm run build
```

如果涉及 Tauri：

必须进行实际 Tauri release 构建验证。

如果涉及具体平台：

必须尽可能进行对应平台实际构建。

最终发布版本必须进入：

```text
Release\<Platform>\
```

不要因为开发环境能够运行就认为 Release 合格。

---

## 十四、真实硬件与 Mock

如果当前平台具备真实硬件，应优先进行真实硬件验证。

同时保留 Mock / fixture 用于：

* Unit Test
* Integration Test
* CI
* 无硬件环境

不能使用 Mock 数据冒充真实硬件测试结果。

---

## 十五、安全与凭据

真实：

* Token
* Device ID
* API Key
* 密码
* 私密配置

不得硬编码到：

* Rust 源码
* TypeScript
* 测试 fixture
* 文档
* Git
* 构建产物

真实测试凭据应通过环境变量或其他安全注入方式提供。

测试完成后及时清理环境变量。

---

## 十六、会话文档是强制要求

**每次完成修改后，都必须更新项目会话文档。**

至少更新：

```text
docs/CONTEXT.md
```

如果本次修改涉及架构、协议、UI、平台、开发流程或其他对应文档，也必须同步更新相关文档，例如：

```text
docs/ARCHITECTURE.md
docs/PROTOCOL.md
docs/WEBSOCKET.md
docs/UI.md
docs/PLATFORM.md
docs/DEVELOPMENT.md
docs/CHANGELOG.md
```

`docs/CONTEXT.md` 必须让下一个 Agent 能够在**最短时间内恢复项目上下文**，至少记录：

* 当前正在进行的 Phase
* 本轮完成了什么
* 修改了哪些核心文件
* 当前实际架构
* 已实现功能
* 已验证内容
* 测试结果
* 构建结果
* Release 构建结果
* 已知限制
* 未完成事项
* 当前工作区状态
* Git commit
* 下一步应该做什么
* 当前是否允许进入下一个 Phase

**不要只写一句“已完成”。必须记录实际状态。**

同时，在开始下一次修改前：

**必须先读取 `docs/CONTEXT.md`，并根据其中的“下一步”和“当前限制”继续工作。**

---

## 十七、修改原则

修改时：

1. 先阅读现有实现和文档。
2. 不重复实现已经存在的功能。
3. 不随意改变既有协议。
4. 不破坏已有平台。
5. 不为了测试方便绕过 Runtime / Dispatcher / Worker 架构。
6. 不用假数据冒充真实功能。
7. 不为了“通过测试”删除或弱化测试。
8. 不把尚未验证的功能标记为 Complete。
9. 修改后运行相关测试。
10. 修改后更新会话文档。
11. 必要时创建清晰的 Git commit。
12. 最终明确说明实际完成、未完成、验证结果和下一步。

---

# 当前任务

**从这里开始，是本次实际需要执行的修改需求：**

完成用户要求修改的几项内容。

要求严格按照上述项目规范执行。

完成修改后：

1. 验证代码。
2. 验证 UI。
3. 如涉及发布，构建正式 Release。
4. 将最终可执行文件放入 `Release\<Platform>\`。
5. 更新 `docs/CONTEXT.md` 及相关文档。
6. 检查 Git 工作区。
7. 汇总实际完成情况。
8. 不夸大完成度，不将未验证内容描述为已验证。
