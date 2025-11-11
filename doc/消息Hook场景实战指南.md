# 消息 Hook 场景实战指南

> 本文基于 `doc/消息管道扩展指南.md` 中的 Hook 体系设计，列举常用的消息校验与通知案例，帮助业务团队快速落地零侵入扩展。
>
> **执行位置**：`flare-message-orchestrator` 在处理 `StoreMessage`/`BatchStoreMessage` 时自动执行 PreSend/PostSend Hook，并通过 Kafka/WAL 流程保证顺序性。配置来源见 `config/hooks.toml`（可被 `MESSAGE_ORCHESTRATOR_HOOKS_CONFIG*` 环境变量覆盖，兼容旧变量）。
>
> **扩展方式对比**：
>
> | 类型 | 适用场景 | 配置方式 | 优点 | 注意事项 |
> |------|----------|----------|------|----------|
> | gRPC Hook | 内网服务、需要严密控制、低延迟 | `type = "grpc"`，指定 endpoint | 强类型、易观测、可水平扩容 | 需保持双端协议一致、注意超时重试 |
> | WebHook | 第三方系统、轻量级接入 | `type = "webhook"`，配置 URL/Secret | 对端实现成本低、跨语言友好 | 建议启用签名校验、控制超时时间 |
> | Local Plugin | 同进程插件、重度业务逻辑 | `type = "local"`，target 为插件名称 | 零网络开销、可复用现有库 | 需随核心服务部署，注意 panic 隔离 |
>
---

## 1. 前置校验：是否群成员、是否好友、是否黑名单

### 1.1 业务诉求
- **群成员校验**：非成员禁止在群内发言。
- **好友校验**：单聊仅允许互为好友的用户通信。
- **黑名单校验**：黑名单之间禁止发送或接收消息。

### 1.2 gRPC Hook 实现步骤
1. **实现 `HookExtension` gRPC 服务**：
   ```protobuf
   service HookExtension {
     rpc InvokePreSend(PreSendHookRequest) returns (PreSendHookResponse);
     // ...
   }
   ```
   - `PreSendHookRequest.context` 携带 `tenant_id`、`session_id`、`message_type`、`sender_id` 等信息。
   - `PreSendHookRequest.draft` 包含消息草稿，可用于补充会话标签。

2. **示例逻辑（伪代码）**
   ```rust
   async fn invoke_pre_send(req: PreSendHookRequest) -> PreSendHookResponse {
       let ctx = req.context.unwrap();
       let draft = req.draft.unwrap();

       match ctx.session_type.as_str() {
           "group" => ensure_group_member(&ctx.session_id, &ctx.sender_id)?,
           "single" => ensure_friendship(&ctx.sender_id, &draft.metadata["target_user"])?,
           _ => {}
       }

       ensure_not_blocked(&ctx.sender_id, &draft.metadata)?;

       PreSendHookResponse {
           allow: true,
           draft: Some(draft),
           status: Some(ok_status()),
           ..Default::default()
       }
   }
   ```

3. **注册配置**
   ```toml
   [[pre_send]]
   name = "relationship-guard"
   priority = 5
   timeout_ms = 50

   [pre_send.selector]
   tenants = ["default"]
   session_types = ["single", "group"]

   [pre_send.transport]
   type = "grpc"
   endpoint = "https://hooks.relation.svc:7443"
   ```

4. **错误约定**：若校验失败，返回 `allow = false`，并在 `status` 中注明 `ErrorCode::OperationFailed` 等业务错误码，客户端立即获得失败原因。

### 1.3 WebHook 版本
1. **接口约定**：核心会向配置的 URL 发送 `POST` 请求，`Content-Type: application/json`，正文类似：
   ```json
   {
     "context": { "tenant_id": "tenant-a", "session_id": "sess-1", ... },
     "draft": { "message_id": "...", "payload": "BASE64...", ... },
     "metadata": { "hook_name": "relationship-webhook" }
   }
   ```

2. **响应约定**：
   ```json
   {
     "allow": true,
     "draft": { "message_id": "..." },
     "status": { "code": "OK", "message": "" }
   }
   ```
   - 若返回 `allow=false` 或 HTTP 非 2xx，会被视为失败；可结合 `require_success=false` 配置忽略错误。

3. **安全建议**：
   - 使用 `secret` 生成签名头（例如 `X-Flare-Signature`）。
   - 限定来源 IP 或部署网关进行鉴权。
   - 合理设置 `timeout_ms` 与重试次数。

4. **配置示例**
   ```toml
   [[pre_send]]
   name = "relationship-webhook"
   priority = 15
   timeout_ms = 1000

   [pre_send.transport]
   type = "webhook"
   endpoint = "https://biz.example.com/hooks/pre_send"
   secret = "replace-me"
   ```

### 1.4 Local Plugin（同进程插件）
1. **创建业务插件 crate**（例如 `flare-biz-message-plugin`）并依赖 `flare-im-core`：
   ```toml
   [dependencies]
   flare-im-core = { path = "../flare-im-core" }
   async-trait = "0.1"
   ```

2. **实现 Trait 并注册**：
   ```rust
   use async_trait::async_trait;
   use flare_im_core::hooks::{HookContext, MessageDraft, PreSendDecision, PreSendHook};

   pub struct RelationshipGuard;

   #[async_trait]
   impl PreSendHook for RelationshipGuard {
       async fn handle(&self, ctx: &HookContext, draft: &mut MessageDraft) -> PreSendDecision {
           if violates_policy(ctx).await {
               return PreSendDecision::Reject {
                   error: flare_im_core::error::ErrorBuilder::new(
                       flare_im_core::error::ErrorCode::OperationFailed,
                       "非好友禁止发消息",
                   )
                   .build_error(),
               };
           }
           PreSendDecision::Continue
       }
   }

   pub fn register(factory: &mut flare_im_core::hooks::adapters::DefaultHookFactory) {
       use std::sync::Arc;
       factory.register_pre_send_local("relationship.guard", Arc::new(RelationshipGuard));
   }
   ```

3. **在服务启动时注册插件**（以 `flare-message-orchestrator` 为例）：
   ```rust
   use flare_biz_message_plugin as biz_plugin;

   let mut factory = DefaultHookFactory::new()?;
   biz_plugin::register(&mut factory);
   hook_config.install(registry.clone(), &factory).await?;
   ```

4. **配置启用**：
   ```toml
   [[pre_send]]
   name = "relationship.guard"
   priority = 20

   [pre_send.transport]
   type = "local"
   target = "relationship.guard"
   ```

5. **适用建议**：
   - 适合对延迟敏感、依赖公共库或业务数据库的逻辑。
   - 注意插件中的 panic/阻塞调用，建议配合 `tokio::spawn_blocking`、错误捕获等手段隔离风险。

---

## 2. 群解散 / 退出：先发系统消息，再通知业务

### 2.1 业务诉求
- 群解散或成员退出时，需要先向群内广播系统消息，再通知业务系统做收尾（日志、审批、计费）。
- 要求消息链路一致，确保系统消息与业务通知严格按顺序执行。

### 2.2 建议方案
1. **群解散/退出操作** 视为普通消息发送（命令类型），进入 `flare-im-core` 主流程。
2. **PreSendHook** 负责：
   - 校验操作者权限（群主/管理员）。
   - 向 `draft.headers/metadata` 填充 `operation=dissolve` 或 `operation=quit`，方便下游识别。
3. **PostSendHook** 执行顺序：
   1. 根据 `MessageRecord` 构造系统消息，写入群消息流（可调用存储 Writer 或直接发布到 Kafka）。
   2. 成功后，调用业务 gRPC/WebHook 或本地插件通知业务系统做后续处理（踢出成员、更新看板等）。

### 2.3 PostSendHook 示例
```toml
[[post_send]]
name = "group-lifecycle-post"
priority = 10
require_success = true

[post_send.selector]
session_types = ["group"]
message_types = ["command"]

[post_send.transport]
type = "grpc"
endpoint = "https://hooks.group.svc:7443"
```

```rust
async fn invoke_post_send(req: PostSendHookRequest) -> PostSendHookResponse {
    let ctx = req.context.unwrap();
    let record = req.record.unwrap();

    if ctx.attributes.get("operation") == Some(&"dissolve".to_string()) {
        broadcast_system_message(&record).await?;
        notify_business("group_dissolve", &record).await?;
    } else if ctx.attributes.get("operation") == Some(&"quit".to_string()) {
        broadcast_system_message(&record).await?;
        notify_business("member_quit", &record).await?;
    }

    PostSendHookResponse {
        success: true,
        status: Some(ok_status()),
        ..Default::default()
    }
}
```

> **顺序保证**：PostSendHook 在消息成功持久化后触发，可确保系统消息和业务通知都以持久化结果为准；若业务通知失败，可利用 Hook 配置的重试/死信机制。

---

## 3. 配置与运维提示
- 所有 Hook 均可通过 `config/hooks.toml` 或 etcd 动态配置：
  - `priority` 控制执行顺序。
  - `require_success` 设置失败是否阻断主流程。
  - `timeout_ms` 配合 `max_retries` 控制稳定性。
- gRPC Hook 必须返回 `flare.common.v1.RpcStatus`，与核心错误体系一致。
- **WebHook** 建议启用签名校验 + 来源 IP 控制。
- **Local Plugin** 建议配合 feature flag / 配置中心控制启停，并做好 panic 防护。
- 推荐对每个 Hook 订阅 Prometheus 指标 `hook_execution_duration_seconds`、`hook_failure_total`、`

## 环境与配置
- 默认通过 `config/services/message_orchestrator.toml` 指定 Kafka / Redis Profile（`kafka.message`、`redis.message_wal`）以及 Hook 配置文件路径。
- 环境变量可覆盖文件值，仍兼容旧命名（`STORAGE_*`）。

## 3. 推送链路 Hook 编排

继消息入口的 PreSend/PostSend 之后，推送子系统也串接了同一套 Hook 能力，覆盖“推送前策略→任务入队→实际送达”全链路。

### 3.1 执行位置
- `flare-push/server` 在消费 Kafka `push-messages`/`push-notifications` 时会：
  - 调用 `PreSend Hook`（内部标记为 `message_type = push_message` / `push_notification`），可据此做免打扰、夜间限推、渠道切换、会员优先级等策略，允许直接修改 `draft.payload/headers/metadata`；
  - 成功入队 Kafka `push-tasks` 后触发 `PostSend Hook`，可用于埋点、审计或调用外部通知系统。
- `flare-push/worker` 在在线/离线发送成功后触发 `Delivery Hook`（`ack_type = online_dispatch/offline_dispatch`），便于统计送达率、下发回执、驱动业务闭环。

### 3.2 配置入口
- `config/services/push_server.toml`：声明推送任务消费的 Kafka/Redis Profile，并配置 `hook_config` / `hook_config_dir`。环境变量可通过 `PUSH_SERVER_HOOKS_CONFIG*` 覆盖。
- `config/services/push_worker.toml`：声明任务 Topic、默认租户、Hook 配置路径，对应环境变量 `PUSH_WORKER_HOOKS_CONFIG*`、`PUSH_WORKER_DEFAULT_TENANT_ID` 等。
- 若不配置独立文件，也可直接使用环境变量：
  - `PUSH_SERVER_HOOKS_CONFIG` / `PUSH_WORKER_HOOKS_CONFIG`
  - `PUSH_SERVER_HOOKS_CONFIG_DIR` / `PUSH_WORKER_HOOKS_CONFIG_DIR`
  - 其余 Kafka/Redis 变量保持与消息编排服务一致。

### 3.3 常见场景
| Hook 阶段 | 示例策略 | 实现提示 |
|-----------|-----------|-----------|
| PreSend   | 夜间免打扰、渠道灰度、设备类型过滤 | 根据 `HookContext.tags`（如 `user_id`、`push.message_type`、`header.*`）与 `draft.metadata`（优先级、客户端信息）决定是否拦截，并可调整 `draft.payload` 实现模板动态渲染 |
| PostSend  | 任务落库、审计、跨域通知 | 读取 `MessageRecord.extra` & `MessageRecord.business_type` 上报 BI、调用运营系统或写入审计链路 |
| Delivery  | 成功率统计、外部回执、营销归因 | `DeliveryEvent` 携带 `message_id`、`user_id`、`ack_type`，可按需分发到监控、计费或增长系统 |

> **提示**：推送链路沿用了主消息链路的 `hooks.toml` 解析逻辑，可通过 selector（如 `message_types = ["push_message"]`）与租户隔离不同策略，也可与消息编排共用配置文件，实现在同一租户内统一管控。
