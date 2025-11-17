//! Gateway Handler - 处理统一网关的请求转发
//! 
//! 注意：communication_core.proto 已删除
//! 业务系统应该使用 AccessGateway 接口推送消息
//! 如果需要统一网关功能，可以聚合多个服务的gRPC接口

use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use flare_proto::TenantContext;
// 注意：communication_core.proto 已删除
// 需要使用对应的proto类型替代
use flare_proto::push::{
    PushMessageRequest as PushProtoMessageRequest,
    PushNotificationRequest as PushProtoNotificationRequest,
};
use flare_proto::signaling::{
    GetOnlineStatusRequest as SignalingGetOnlineStatusRequest,
    LoginRequest as SignalingLoginRequest, OnlineStatus as SignalingOnlineStatus,
    RouteMessageRequest as SignalingRouteMessageRequest,
    RouteMessageResponse as SignalingRouteMessageResponse,
};
use flare_proto::storage::{
    QueryMessagesRequest as StorageQueryMessagesRequest,
    StoreMessageRequest as StorageStoreMessageRequest,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};

use crate::infrastructure::push::PushClient;
use crate::infrastructure::signaling::SignalingClient;
use crate::infrastructure::storage::StorageClient;

pub struct GatewayHandler {
    signaling: Arc<dyn SignalingClient>,
    storage: Arc<dyn StorageClient>,
    push: Arc<dyn PushClient>,
}

pub(crate) fn tenant_id_label(tenant: Option<&TenantContext>) -> &str {
    tenant
        .and_then(|ctx| {
            let trimmed = ctx.tenant_id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or("tenant:undefined")
}

fn ensure_tenant(tenant: &Option<TenantContext>, operation: &str) -> Result<()> {
    match tenant {
        Some(ctx) if !ctx.tenant_id.trim().is_empty() => Ok(()),
        _ => Err(
            ErrorBuilder::new(ErrorCode::InvalidParameter, "tenant context is required")
                .details(format!("operation={operation}"))
                .build_error(),
        ),
    }
}

fn to_signaling_login(request: SignalingLoginRequest) -> SignalingLoginRequest {
    // 直接返回，因为类型已经匹配
    request
}

// 注意：communication_core.proto 已删除，以下函数已注释
// fn to_core_online_status(status: SignalingOnlineStatus) -> flare_proto::communication_core::OnlineStatus { ... }
// fn convert_push_failure(failure: flare_proto::push::PushFailure) -> flare_proto::communication_core::PushFailure { ... }

impl GatewayHandler {
    pub fn new(
        signaling: Arc<dyn SignalingClient>,
        storage: Arc<dyn StorageClient>,
        push: Arc<dyn PushClient>,
    ) -> Self {
        Self {
            signaling,
            storage,
            push,
        }
    }

    // 注意：communication_core.proto 已删除，以下方法已注释
    // 如果需要统一网关功能，可以聚合多个服务的gRPC接口
    // pub async fn handle_login(&self, request: LoginRequest) -> Result<LoginResponse> { ... }
    // pub async fn handle_get_online_status(&self, request: GetOnlineStatusRequest) -> Result<GetOnlineStatusResponse> { ... }
    // pub async fn handle_route_message(&self, request: RouteMessageRequest) -> Result<RouteMessageResponse> { ... }
    // pub async fn handle_store_message(&self, request: StoreMessageRequest) -> Result<StoreMessageResponse> { ... }
    // pub async fn handle_query_messages(&self, request: QueryMessagesRequest) -> Result<QueryMessagesResponse> { ... }
    // pub async fn handle_push_message(&self, request: PushMessageRequest) -> Result<PushMessageResponse> { ... }
    // pub async fn handle_push_notification(&self, request: PushNotificationRequest) -> Result<PushNotificationResponse> { ... }
}
