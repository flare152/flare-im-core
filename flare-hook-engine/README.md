# Flare Hook Engine

Flare IM Hook引擎 - 统一的Hook配置管理和执行调度引擎

## 核心职责

- **配置管理**：支持配置文件、动态API（数据库）、配置中心三种配置方式
- **执行调度**：按优先级执行Hook，支持并发执行和超时控制
- **监控统计**：收集Hook执行指标，提供监控和告警能力
- **扩展机制**：支持gRPC、WebHook、Local Plugin三种扩展方式

## Hook配置方式

Hook引擎支持三种配置方式，按优先级从高到低排序：

### 1. 动态API配置（最高优先级）

**存储位置**：PostgreSQL数据库

**特点**：
- 支持多租户隔离
- 支持动态增删改查
- 支持版本管理
- 支持审计日志（记录创建者、创建时间、更新时间）

**数据库表结构**：
```sql
CREATE TABLE hook_configs (
    id BIGSERIAL PRIMARY KEY,
    tenant_id TEXT,                    -- 租户ID（可选，用于多租户隔离）
    hook_type TEXT NOT NULL,            -- Hook类型（pre_send, post_send等）
    name TEXT NOT NULL,                 -- Hook名称
    version TEXT,                       -- Hook版本
    description TEXT,                   -- Hook描述
    enabled BOOLEAN NOT NULL DEFAULT true,
    priority INTEGER NOT NULL DEFAULT 100,
    group_name TEXT,                    -- Hook分组（validation/critical/business）
    timeout_ms BIGINT NOT NULL DEFAULT 1000,
    max_retries INTEGER NOT NULL DEFAULT 0,
    error_policy TEXT NOT NULL DEFAULT 'fail_fast',
    require_success BOOLEAN NOT NULL DEFAULT true,
    selector_config JSONB NOT NULL DEFAULT '{}',  -- 选择器配置
    transport_config JSONB NOT NULL,               -- 传输配置
    metadata JSONB,                                -- 元数据
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,                               -- 创建者
    
    UNIQUE(tenant_id, hook_type, name)  -- 唯一约束
);
```

**使用方式**：
```rust
use flare_hook_engine::service::HookEngineConfig;

let config = HookEngineConfig {
    database_url: Some("postgresql://user:pass@localhost:5432/flare".to_string()),
    tenant_id: Some("tenant-a".to_string()),
    config_file: Some("config/hooks.toml".into()),
    config_center_endpoint: Some("etcd://localhost:2379".to_string()),
    ..Default::default()
};

let engine = HookEngine::new(config).await?;
```

**配置管理API**（通过PostgresHookConfigRepository）：
```rust
// 保存Hook配置
repository.save(
    Some("tenant-a"),
    "pre_send",
    &hook_config_item,
    Some("admin")
).await?;

// 删除Hook配置
repository.delete(
    Some("tenant-a"),
    "pre_send",
    "hook-name"
).await?;
```

### 2. 配置中心配置（中等优先级）

**存储位置**：etcd 或 Consul

**特点**：
- 支持多租户隔离
- 支持配置变更监听（watch）
- 适合分布式环境

**配置键格式**：
- 多租户：`/flare/hooks/{tenant_id}/config`
- 全局：`/flare/hooks/config`

**使用方式**：
```rust
let config = HookEngineConfig {
    config_center_endpoint: Some("etcd://localhost:2379".to_string()),
    tenant_id: Some("tenant-a".to_string()),
    ..Default::default()
};
```

**配置格式**（JSON）：
```json
{
  "pre_send": [
    {
      "name": "friendship-check",
      "priority": 5,
      "timeout_ms": 50,
      "enabled": true,
      "require_success": true,
      "selector": {
        "tenants": ["tenant-a"],
        "conversation_types": ["single"]
      },
      "transport": {
        "type": "grpc",
        "endpoint": "https://hooks.relation.svc:7443"
      }
    }
  ]
}
```

### 3. 配置文件配置（最低优先级）

**存储位置**：本地TOML文件

**特点**：
- 适合开发环境
- 适合静态配置
- 不支持动态更新（需要重启服务）

**配置文件路径**：`config/hooks.toml`

**配置示例**：
```toml
[[pre_send]]
name = "friendship-check"
priority = 5
timeout_ms = 50
enabled = true
require_success = true

[pre_send.selector]
tenants = ["tenant-a"]
conversation_types = ["single"]
message_types = ["text", "image"]

[pre_send.transport]
type = "grpc"
endpoint = "https://hooks.relation.svc:7443"

[pre_send.retry]
max_retries = 3
retry_interval_ms = 100
backoff_strategy = "exponential"
```

## 配置优先级和合并规则

1. **优先级顺序**：数据库配置 > 配置中心配置 > 配置文件配置
2. **合并规则**：同名Hook配置，高优先级覆盖低优先级
3. **加载顺序**：按优先级从低到高加载，最后加载的配置会覆盖前面的同名配置

## 多租户支持

Hook引擎支持多租户场景：

- **租户隔离**：每个租户可以有独立的Hook配置
- **全局配置**：`tenant_id`为`NULL`的配置对所有租户生效
- **租户配置**：特定租户的配置会覆盖全局配置

**查询逻辑**：
```sql
-- 查询租户配置时，会同时返回：
-- 1. 全局配置（tenant_id IS NULL）
-- 2. 租户特定配置（tenant_id = 'tenant-a'）
SELECT * FROM hook_configs
WHERE enabled = true
  AND (tenant_id IS NULL OR tenant_id = 'tenant-a')
ORDER BY hook_type, priority ASC
```

## 配置刷新

Hook引擎支持配置热刷新：

- **刷新间隔**：默认60秒（可通过`refresh_interval_secs`配置）
- **自动刷新**：定时从所有配置源重新加载配置
- **配置验证**：刷新时会验证配置格式，无效配置会被忽略

## 使用示例

### 基本使用

```rust
use flare_hook_engine::service::{HookEngine, HookEngineConfig};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 创建Hook引擎配置
    let config = HookEngineConfig {
        // 配置文件（可选）
        config_file: Some("config/hooks.toml".into()),
        
        // 数据库配置（可选，最高优先级）
        database_url: Some(
            std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "postgresql://flare:flare123@localhost:25432/flare".to_string())
        ),
        
        // 配置中心（可选，中等优先级）
        config_center_endpoint: Some("etcd://localhost:22379".to_string()),
        
        // 租户ID（可选）
        tenant_id: Some("tenant-a".to_string()),
        
        // 配置刷新间隔（秒）
        refresh_interval_secs: 60,
        
        ..Default::default()
    };
    
    // 创建Hook引擎
    let engine = HookEngine::new(config).await?;
    
    // 使用Hook引擎执行Hook
    let ctx = HookContext::new("tenant-a");
    let mut draft = MessageDraft::new(b"test message".to_vec());
    
    // 执行PreSend Hook
    let decision = engine.command_service
        .pre_send(&ctx, &mut draft, hooks)
        .await?;
    
    Ok(())
}
```

### 数据库配置管理

```rust
use flare_hook_engine::infrastructure::persistence::PostgresHookConfigRepository;
use flare_hook_engine::domain::models::{HookConfigItem, HookSelectorConfig, HookTransportConfig};

// 创建配置仓储
let repository = PostgresHookConfigRepository::new(
    "postgresql://user:pass@localhost:5432/flare"
).await?;

// 初始化数据库表
repository.init_schema().await?;

// 创建Hook配置
let hook_config = HookConfigItem {
    name: "friendship-check".to_string(),
    priority: 5,
    timeout_ms: 50,
    enabled: true,
    require_success: true,
    selector: HookSelectorConfig {
        tenants: vec!["tenant-a".to_string()],
        conversation_types: vec!["single".to_string()],
        ..Default::default()
    },
    transport: HookTransportConfig::Grpc {
        endpoint: "https://hooks.relation.svc:7443".to_string(),
        metadata: HashMap::new(),
    },
    ..Default::default()
};

// 保存配置
let id = repository.save(
    Some("tenant-a"),
    "pre_send",
    &hook_config,
    Some("admin")
).await?;
```

## 架构设计

Hook引擎采用DDD + CQRS架构：

```
flare-hook-engine/
├── domain/              # 领域层
│   ├── models.rs        # Hook配置模型、执行计划
│   ├── repositories.rs  # 仓储接口
│   └── service.rs       # 领域服务（编排服务）
├── application/         # 应用层
│   ├── commands.rs      # Hook执行命令
│   ├── queries.rs       # Hook查询服务
│   └── service.rs       # 应用服务门面
├── infrastructure/      # 基础设施层
│   ├── adapters/        # Hook适配器（gRPC、WebHook、Local）
│   ├── config/          # 配置加载器（文件、数据库、配置中心）
│   ├── monitoring/      # 监控统计
│   └── persistence/     # 配置持久化（PostgreSQL）
├── interface/           # 接口层
│   └── grpc/            # gRPC服务接口
└── service/             # 服务层
    ├── bootstrap.rs     # 应用启动器
    └── registry.rs      # 服务注册
```

## 配置项说明

### Hook配置项（HookConfigItem）

| 字段 | 类型 | 说明 | 默认值 |
|------|------|------|--------|
| `name` | String | Hook名称（必填） | - |
| `priority` | i32 | 优先级（0-1000，越小越高） | 100 |
| `timeout_ms` | u64 | 超时时间（毫秒） | 1000 |
| `enabled` | bool | 是否启用 | true |
| `require_success` | bool | 是否要求成功 | true |
| `error_policy` | String | 错误策略（fail_fast/retry/ignore） | fail_fast |
| `max_retries` | u32 | 最大重试次数 | 0 |
| `selector` | HookSelectorConfig | 选择器配置 | - |
| `transport` | HookTransportConfig | 传输配置 | - |

### Hook选择器（HookSelectorConfig）

用于匹配Hook执行条件：

| 字段 | 类型 | 说明 |
|------|------|------|
| `tenants` | Vec<String> | 租户列表（空表示匹配所有租户） |
| `conversation_types` | Vec<String> | 会话类型列表（空表示匹配所有类型） |
| `message_types` | Vec<String> | 消息类型列表（空表示匹配所有类型） |
| `user_ids` | Vec<String> | 用户ID列表（空表示匹配所有用户） |
| `tags` | HashMap<String, String> | 标签匹配 |

### Hook传输配置（HookTransportConfig）

支持三种传输方式：

1. **gRPC传输**：
   ```toml
   [transport]
   type = "grpc"
   endpoint = "https://hooks.example.com:7443"
   ```

2. **WebHook传输**：
   ```toml
   [transport]
   type = "webhook"
   endpoint = "https://hooks.example.com/webhook"
   secret = "your-secret-key"
   ```

3. **Local Plugin传输**：
   ```toml
   [transport]
   type = "local"
   target = "plugin-name"
   ```

## 监控和统计

Hook引擎提供以下监控指标：

- `hook_execution_total`：Hook执行总次数
- `hook_execution_duration_seconds`：Hook执行耗时
- `hook_success_rate`：Hook成功率
- `hook_failure_total`：Hook失败次数
- `hook_timeout_total`：Hook超时次数

## 参考文档

- [Hook可配置点与业务处理设计](../doc/Hook可配置点与业务处理设计.md)
- [Hook引擎集成指南](../doc/Hook引擎集成指南.md)
- [Hook编排机制设计](../doc/Hook编排机制设计.md)

