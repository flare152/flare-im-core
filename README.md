# Flare IM Core

Flare IM 通信核心层提供完整的 IM 通信基础设施，遵循文档目录 [doc/ARCHITECTURE_INDEX.md](../doc/ARCHITECTURE_INDEX.md) 中定义的架构蓝图。代码与文档同步演进，采用 DDD + CQRS 分层，所有 gRPC 接口遵循 `flare-proto` 统一协议。

## 架构概览

| 服务/目录 | 角色 | 关键职责 | 关联文档 |
|-----------|------|----------|----------|
| `flare-core-gateway` | 外部统一入口 | 聚合 Signaling / Storage / Push / Media 的 gRPC 服务，处理上下文与租户信息透传 | [CORE_COMMUNICATION_LAYER.md](../doc/CORE_COMMUNICATION_LAYER.md) |
| `flare-access-gateway` | 客户端接入层 | WebSocket/QUIC 长连接 + gRPC 会话接口，认证、心跳、路由、消息推送 | [ARCHITECTURE_GATEWAY.md](./ARCHITECTURE_GATEWAY.md) |
| `flare-signaling/online` | 在线状态 | 与 gateway 协同的用户会话登记、心跳续期、在线状态查询 | [SYSTEM_DECOMPOSITION.md](../doc/SYSTEM_DECOMPOSITION.md) |
| `flare-signaling/route` | 业务路由 | SVID → Endpoint 的业务路由目录，供 gateway 与业务侧互联 | 同上 |
| `flare-push/*` | 推送链路 | Proxy 入队 → Server 判定在线 → Worker 执行（信令内推 / 第三方离线） | [MODULE_DESIGN.md](../doc/MODULE_DESIGN.md) |
| `flare-message-orchestrator` | 消息编排 | PreSend 强校验 + Kafka 事件入队，统一驱动存储/推送等订阅者 | [消息上下行调度架构](doc/消息上下行调度架构.md) |
| `flare-storage/*` | 消息存储 | Writer 持久化 → Reader 查询侧（消费 orchestrator 事件） | [MESSAGE_STORAGE_DESIGN.md](../doc/MESSAGE_STORAGE_DESIGN.md) |
| `flare-media` | 媒资服务 | 对象存储、元数据、转码流程 | [MODULE_DESIGN.md](../doc/MODULE_DESIGN.md) |
| `flare-core-sdk/server` | 服务端 SDK | gRPC 客户端封装，与业务服务协同 | [SDK_DESIGN.md](../doc/SDK_DESIGN.md) |
| `消息扩展文档` | Hook 体系 | 业务零侵入扩展消息链路的实践 | [消息管道扩展指南](doc/消息管道扩展指南.md) |
| `Hook 使用范例` | 常见场景 | 群成员/好友/黑名单校验、群解散/退出通知流程 | [消息 Hook 场景实战指南](doc/消息Hook场景实战指南.md) |
| `调度架构设计` | 消息上下行调度 | 接入 → 调度 → Kafka → 订阅者 → Route 的整体设计 | [消息上下行调度架构](doc/消息上下行调度架构.md) |
| `模块职责概览` | 系统拆解 | 汇总各服务职责、依赖与 Hook 布局 | [模块职责概览](doc/模块职责概览.md) |

> 更完整的系统容量规划、扩散策略与全球多活部署说明可参考 [ARCHITECTURE_SUMMARY.md](../ARCHITECTURE_SUMMARY.md)。

详细服务交互、部署建议与性能指标可在 [CORE_ARCHITECTURE.md](../doc/CORE_ARCHITECTURE.md) 与 [HIGH_AVAILABILITY.md](../doc/HIGH_AVAILABILITY.md) 中查阅。

## 代码结构与职责

- **分层约定**：`application/` 调度用例，`domain/` 声明领域模型与仓储接口，`infrastructure/` 对接外部系统（Redis、Kafka、gRPC 等），`handler/` 暴露 gRPC / 长连接接口。
- **协议约定**：所有服务通过 `flare-proto` 生成的类型交互，统一携带 `RequestContext`、`TenantContext`、`RpcStatus`。
- **配置约定**：环境变量说明见 [SERVICE_CONFIGURATION.md](../doc/SERVICE_CONFIGURATION.md)。

```text
flare-im-core/
├── flare-access-gateway/     # 接入层，基于 flare-core 提供长连接
├── flare-signaling/          # 信令子系统：online / route
├── flare-push/               # 推送子系统：proxy / server / worker
├── flare-message-orchestrator/ # 消息编排服务，执行 Hook 校验与事件入队
├── flare-storage/            # 存储子系统：model / writer / reader
├── flare-media/              # 媒资子系统
└── flare-core-gateway/       # 对外统一 gRPC Gateway
```

## 关键链路

1. **客户端接入**：`flare-access-gateway` 负责 token 认证、连接管理，调用 `flare-signaling-online` 登记会话并周期性心跳；消息经长连接进入 `flare-core-gateway` 再分发至存储/推送。
2. **消息写入/读取**：客户端消息经 `flare-core-gateway → flare-message-orchestrator → Kafka → flare-storage-writer` 完成持久化，读取由 `flare-storage-reader` 根据游标命中 Redis / MongoDB / TimescaleDB。
3. **推送与通知**：`flare-core-gateway` 将推送请求投递至 `flare-push-proxy`，由 `flare-push-server` 判定在线并派发任务，`flare-push-worker` 执行在线信令推送或第三方离线通道。

流程细节请参见 [doc/消息流程图_v2.drawio](../doc/消息流程图_v2.drawio) 与 [BUSINESS_SYSTEM_INTEGRATION.md](../doc/BUSINESS_SYSTEM_INTEGRATION.md)。

## 模块启动命令

所有命令均在 `flare-im-core/` 根目录执行，默认依赖共享配置 `config/base.toml` 及对应服务的环境变量（见 [SERVICE_CONFIGURATION.md](../doc/SERVICE_CONFIGURATION.md)）。可按需结合 `cargo watch` 或 `bacon` 等工具增量编译。

| 子系统 | 模块 | 启动命令 |
|--------|------|---------|
| Access Gateway | 接入网关 | `cargo run --bin flare-access-gateway` |
| Core Gateway | 外部统一 gRPC 网关 | `cargo run --bin flare-core-gateway` |
| Signaling | 在线状态服务 | `cargo run --bin flare-signaling-online` |
| Signaling | 路由目录服务 | `cargo run --bin flare-signaling-route` |
| Message | 消息编排服务 | `cargo run --bin flare-message-orchestrator` |
| Push | 推送代理（入队） | `cargo run --bin flare-push-proxy` |
| Push | 推送服务（判定在线/派发） | `cargo run --bin flare-push-server` |
| Push | 推送 worker（执行发送） | `cargo run --bin flare-push-worker` |
| Storage | 存储 Writer（持久化） | `cargo run --bin flare-storage-writer` |
| Storage | 存储 Reader（查询侧 gRPC） | `cargo run --bin flare-storage-reader` |
| Media | 媒资服务 | `cargo run --bin flare-media` |

> 如需同时启动多个模块，建议配合 `cargo run -p <crate>` 指定 package，或使用 `just`/`make` 封装脚本。线上环境部署流程详见 [DEPLOYMENT_GUIDE.md](../doc/DEPLOYMENT_GUIDE.md)。

## 配置

所有服务默认读取工作空间根部的 `config/` 目录：`base.toml` 提供共享依赖（Redis、Kafka、对象存储等），`services/*.toml` 描述各自覆盖的运行时参数。可以通过环境变量覆盖关键配置，详见 [SERVICE_CONFIGURATION.md](../doc/SERVICE_CONFIGURATION.md)。

各子服务亦可在自身目录中提供专属 `CONFIG.md` / README（待补充）描述定制化参数。

### 配置体系
- `config/base.toml`：全局配置、Profile 定义（Kafka/Redis 等）。
  - 新增 `kafka.message`、`redis.message_wal` Profile，供消息编排服务复用。
- `config/services/*.toml`：服务级配置，覆盖/组合 Profile。
  - `services/message_orchestrator.toml` 指定 Kafka Topic、WAL 存储和 Hook 配置路径。

## 运行

```bash
# 启动统一接入网关（WebSocket + QUIC + gRPC）
cargo run --bin flare-access-gateway

# 启动统一 gRPC 网关
cargo run --bin flare-core-gateway

# 信令子系统
cargo run --bin flare-signaling-online
cargo run --bin flare-signaling-route

# 消息编排服务
cargo run --bin flare-message-orchestrator

# 推送子系统
cargo run --bin flare-push-proxy
cargo run --bin flare-push-server
cargo run --bin flare-push-worker

# 存储子系统
cargo run --bin flare-storage-writer
cargo run --bin flare-storage-reader

# 媒资服务
cargo run --bin flare-media
```

> 推荐按照 [DEPLOYMENT_GUIDE.md](../doc/DEPLOYMENT_GUIDE.md) 的建议逐步启用依赖服务（Kafka、Redis、MongoDB、TimescaleDB、MinIO 等），并结合 [QUICK_REFERENCE.md](../doc/QUICK_REFERENCE.md) 快速自查常用命令。

## 依赖

- `flare-core`：负责连接管理、协议编解码。
- `flare-server-core`：封装服务注册、配置装载、通用中间件。
- `flare-proto`：统一 gRPC/Protobuf 协议定义。

更多延伸阅读与设计细节，请参考 `doc/` 目录下的各类架构文档。
