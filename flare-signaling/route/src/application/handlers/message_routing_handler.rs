//! 消息路由处理器
//!
//! 负责消息路由的业务流程编排

use std::sync::Arc;
use anyhow::{Context, Result};
use flare_proto::common::{RequestContext, TenantContext};

use crate::domain::service::MessageRoutingDomainService;
use crate::infrastructure::forwarder::MessageForwarder;

/// 消息路由处理器
///
/// 职责：
/// - 编排消息路由流程
/// - 调用领域服务进行路由决策
/// - 调用基础设施层进行消息转发
pub struct MessageRoutingHandler {
    routing_domain_service: Arc<MessageRoutingDomainService>,
    message_forwarder: Arc<MessageForwarder>,
}

impl MessageRoutingHandler {
    pub fn new(
        routing_domain_service: Arc<MessageRoutingDomainService>,
        message_forwarder: Arc<MessageForwarder>,
    ) -> Self {
        Self {
            routing_domain_service,
            message_forwarder,
        }
    }

    /// 路由消息到业务系统
    ///
    /// # 流程
    /// 1. 构建路由上下文
    /// 2. 调用领域服务解析端点
    /// 3. 调用消息转发服务转发消息
    ///
    /// # 返回
    /// (端点地址, 响应数据)
    pub async fn route_message(
        &self,
        svid: &str,
        payload: Vec<u8>,
        context: Option<RequestContext>,
        tenant: Option<TenantContext>,
    ) -> Result<(String, Vec<u8>)> {
        use crate::domain::service::RouteContext;

        // 构建路由上下文
        let route_ctx = RouteContext {
            svid: svid.to_string(),
            conversation_id: context
                .as_ref()
                .and_then(|c| c.attributes.get("conversation_id").cloned()),
            user_id: context
                .as_ref()
                .and_then(|c| c.actor.as_ref().map(|a| a.actor_id.clone())),
            tenant_id: tenant.as_ref().map(|t| t.tenant_id.clone()),
            client_geo: context
                .as_ref()
                .and_then(|c| c.attributes.get("geo").cloned()),
            login_gateway: context
                .as_ref()
                .and_then(|c| c.attributes.get("login_gateway").cloned()),
        };

        // 调用领域服务解析端点（如果需要复杂路由逻辑）
        // 注意：当前 MessageForwarder 内部已经处理了路由，这里可以简化
        // 如果未来需要更复杂的路由决策，可以在这里调用 MessageRoutingDomainService

        // 调用消息转发服务转发消息
        self.message_forwarder
            .forward_message(svid, payload, context, tenant, None)
            .await
            .context("Failed to forward message")
    }
}

