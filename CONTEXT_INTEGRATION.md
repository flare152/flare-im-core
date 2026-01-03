# 上下文集成指南

本文档说明如何在 `flare-im-core` 的所有服务中使用上下文能力。

## 概述

`flare-server-core` 提供了统一的上下文类型系统（`TenantContext`、`RequestContext`），通过 Tower 中间件自动提取和注入上下文信息。

## 服务端集成

### 1. 在 Bootstrap 中添加中间件

所有 gRPC 服务的 bootstrap 文件都应该添加 `TenantLayer` 和 `RequestContextLayer`：

```rust
use flare_server_core::middleware::{RequestContextLayer, TenantLayer};

Server::builder()
    .layer(TenantLayer::new().allow_missing(true)) // 允许缺失，兼容旧代码
    .layer(RequestContextLayer::new().allow_missing(true))
    .add_service(YourServiceServer::new(handler))
    // ...
```

### 2. 在 Handler 中提取上下文

#### 推荐方式：使用 flare-server-core 的工具函数

```rust
use flare_server_core::middleware::{
    extract_tenant_context, extract_request_context, extract_tenant_id, extract_user_id,
};
use tonic::{Request, Response, Status};

async fn my_handler(req: Request<MyRequest>) -> Result<Response<MyResponse>, Status> {
    // 方式1：提取完整的 TenantContext
    let tenant = extract_tenant_context(&req)
        .ok_or_else(|| Status::invalid_argument("TenantContext required"))?;
    println!("Tenant ID: {}", tenant.tenant_id);
    
    // 方式2：只提取 tenant_id（便捷方法）
    let tenant_id = extract_tenant_id(&req)
        .ok_or_else(|| Status::invalid_argument("Tenant ID required"))?;
    
    // 方式3：提取 RequestContext
    if let Some(req_ctx) = extract_request_context(&req) {
        println!("Request ID: {}", req_ctx.request_id);
        if let Some(user_id) = req_ctx.user_id() {
            println!("User ID: {}", user_id);
        }
    }
    
    // 方式4：直接提取 user_id（便捷方法）
    let user_id = extract_user_id(&req);
    
    // ...
}
```

#### 使用 flare-im-core 的便捷函数

```rust
use flare_im_core::{extract_tenant_from_request, extract_tenant_id_from_request};

async fn my_handler(req: Request<MyRequest>) -> Result<Response<MyResponse>, Status> {
    // 提取 TenantContext
    let tenant = extract_tenant_from_request(&req)
        .ok_or_else(|| Status::invalid_argument("TenantContext required"))?;
    
    // 提取 tenant_id
    let tenant_id = extract_tenant_id_from_request(&req);
    
    // ...
}
```

### 3. 兼容旧代码

如果请求中包含 proto 类型的 `TenantContext` 或 `RequestContext`，可以使用兼容函数：

```rust
use flare_im_core::utils::context::{extract_or_build_tenant_context, extract_or_build_request_context};

async fn my_handler(req: Request<SendMessageRequest>) -> Result<Response<SendMessageResponse>, Status> {
    let req_inner = req.into_inner();
    
    // 优先从请求扩展中提取，如果没有则从 proto 转换
    let tenant = extract_or_build_tenant_context(&req, req_inner.tenant.as_ref())
        .ok_or_else(|| Status::invalid_argument("TenantContext required"))?;
    
    // ...
}
```

## 客户端集成

### 1. 手动设置上下文

```rust
use flare_server_core::client::{set_tenant_context, set_tenant_id};
use flare_server_core::TenantContext;

let mut request = Request::new(my_request);

// 方式1：设置完整的 TenantContext
let tenant = TenantContext::new("tenant-123")
    .with_business_type("im")
    .with_environment("production");
set_tenant_context(&mut request, &tenant);

// 方式2：只设置 tenant_id（便捷方法）
set_tenant_id(&mut request, "tenant-123");
```

### 2. 使用 RequestBuilder（链式 API）

```rust
use flare_server_core::client::RequestBuilder;
use flare_server_core::TenantContext;

let tenant = TenantContext::new("tenant-123");

let request = RequestBuilder::new(my_request)
    .with_tenant(&tenant)
    .with_user_id("user-456")
    .build();
```

### 3. 使用 ClientContextInterceptor（推荐，自动设置）

```rust
use flare_server_core::client::{ClientContextInterceptor, ClientContextConfig};
use flare_server_core::TenantContext;

// 创建配置
let config = ClientContextConfig::new()
    .with_default_tenant(TenantContext::new("tenant-123"))
    .with_default_user_id("user-456".to_string());

// 创建拦截器
let interceptor = ClientContextInterceptor::new(config);

// 应用到客户端
let client = YourServiceClient::with_interceptor(channel, interceptor);

// 现在所有请求都会自动包含租户上下文和用户ID
```

## 已更新的服务

以下服务的 bootstrap 文件已经添加了上下文中间件：

- ✅ `flare-message-orchestrator` - MessageService
- ✅ `flare-signaling/gateway` - AccessGatewayService
- ✅ `flare-hook-engine` - HookExtensionService, HookService
- ✅ `flare-conversation` - ConversationService
- ✅ `flare-signaling/online` - OnlineService
- ✅ `flare-media` - MediaService
- ✅ `flare-signaling/route` - RouterService
- ✅ `flare-core-gateway` - 所有代理服务
- ✅ `flare-push/proxy` - PushService
- ✅ `flare-storage/reader` - StorageReaderService

## 迁移指南

### 从手动 metadata 提取迁移

**旧代码**：
```rust
fn extract_tenant_id<T>(request: &Request<T>) -> Option<String> {
    request
        .metadata()
        .get("x-tenant-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}
```

**新代码**：
```rust
use flare_server_core::middleware::extract_tenant_id;

fn extract_tenant_id<T>(request: &Request<T>) -> Option<String> {
    extract_tenant_id(request)
}
```

### 从 proto 类型迁移

**旧代码**：
```rust
let tenant_id = req.tenant.as_ref().map(|t| t.tenant_id.clone());
```

**新代码**：
```rust
use flare_im_core::extract_tenant_id_from_request;

let tenant_id = extract_tenant_id_from_request(&req);
// 或者
let tenant = extract_tenant_from_request(&req);
let tenant_id = tenant.map(|t| t.tenant_id.clone());
```

## 最佳实践

1. **服务端**：始终在 bootstrap 中添加 `TenantLayer` 和 `RequestContextLayer`
2. **Handler**：优先使用 `extract_tenant_context` 等工具函数，而不是手动从 metadata 提取
3. **客户端**：推荐使用 `ClientContextInterceptor` 自动设置上下文
4. **错误处理**：使用 `require_tenant_context` 确保必需上下文存在
5. **兼容性**：如果请求中包含 proto 类型的上下文，使用 `extract_or_build_*` 函数

## 注意事项

1. 中间件设置为 `allow_missing(true)` 以兼容旧代码，未来可以改为 `false` 强制要求
2. 上下文信息存储在请求扩展中，访问成本低
3. 如果同时存在请求扩展和 proto 字段，优先使用请求扩展中的上下文

