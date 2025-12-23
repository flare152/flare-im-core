//! 消息处理处理器
//!
//! 处理消息收发的业务流程编排

use flare_core::common::error::{FlareError, Result};
use flare_core::common::protocol::MessageCommand;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};
use crate::infrastructure::ConversationServiceClient;
use crate::domain::service::{ConversationDomainService, MessageDomainService};
use crate::infrastructure::AckPublisher;
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::infrastructure::messaging::message_router::MessageRouter;

/// 消息处理处理器（应用层 - 编排层）
///
/// 职责：
/// - 编排消息接收流程（调用领域服务）
/// - 编排消息发送流程（调用基础设施）
/// - 编排 ACK 处理流程（调用基础设施）
/// 
/// # 注意
/// 此处理器不包含业务逻辑，业务逻辑在 MessageDomainService 中
pub struct MessageHandler {
    message_domain_service: Arc<MessageDomainService>,
    message_router: Option<Arc<MessageRouter>>,
    ack_sender: Arc<AckSender>,
    ack_publisher: Option<Arc<dyn AckPublisher>>,
    session_domain_service: Arc<ConversationDomainService>,
    conversation_service_client: Option<Arc<ConversationServiceClient>>,
    gateway_id: String,
}

impl MessageHandler {
    pub fn new(
        message_domain_service: Arc<MessageDomainService>,
        message_router: Option<Arc<MessageRouter>>,
        ack_sender: Arc<AckSender>,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        session_domain_service: Arc<ConversationDomainService>,
        conversation_service_client: Option<Arc<ConversationServiceClient>>,
        gateway_id: String,
    ) -> Self {
        Self {
            message_domain_service,
            message_router,
            ack_sender,
            ack_publisher,
            session_domain_service,
            conversation_service_client,
            gateway_id,
        }
    }

    /// 处理消息发送
    ///
    /// 流程：
    /// 1. 从 payload 中提取 conversation_id
    /// 2. 路由消息到 Message Orchestrator
    /// 3. 发送 ACK 到客户端
    ///
    /// # 返回值
    /// 返回服务端生成的消息 ID（server_id），用于在 ACK 中返回给 SDK
    #[instrument(skip(self, msg_cmd), fields(connection_id, user_id, message_id = %msg_cmd.message_id))]
    pub async fn handle_message_send(
        &self,
        connection_id: &str,
        user_id: &str,
        msg_cmd: &MessageCommand,
        tenant_id: Option<&str>,
    ) -> Result<(String,u64)> {
        let start_time = std::time::Instant::now();
        debug!(
            user_id,
            connection_id = %connection_id,
            message_id = %msg_cmd.message_id,
            message_len = msg_cmd.payload.len(),
            "Message received from client"
        );
        // 检查消息路由器
        let router = self.message_router.as_ref().ok_or_else(|| {
            let error_msg = "Message Router not configured";
            error!(
                user_id = %user_id,
                connection_id = %connection_id,
                message_id = %msg_cmd.message_id,
                "Message Router not configured"
            );
            FlareError::system(error_msg)
        })?;

        // 验证消息格式（领域层业务规则）
        if let Err(e) = self.message_domain_service.validate_message(&msg_cmd) {
            let error_msg = format!("无效的消息格式: {}", e);
            error!(
                ?e,
                user_id = %user_id,
                connection_id = %connection_id,
                message_id = %msg_cmd.message_id,
                "Failed to validate message"
            );
            return Err(e);
        }

        // 提取 conversation_id（领域层业务逻辑）
        let conversation_id = match self.message_domain_service.extract_conversation_id(&msg_cmd) {
            Ok(sid) => sid,
            Err(e) => {
                let error_msg = format!("无效的消息格式: {}", e);
                error!(
                    ?e,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    message_id = %msg_cmd.message_id,
                    "Failed to extract conversation_id from message"
                );
                // 返回错误，不再继续处理
                return Err(e);
            }
        };

        // 路由消息
        let original_message_id = msg_cmd.message_id.clone();
        let route_res = router
            .route_message(user_id, &conversation_id, msg_cmd.payload.clone(), tenant_id)
            .await;

        let route_duration = start_time.elapsed();
        match route_res {
            Ok(response) => {
                Ok((response.server_msg_id.clone(),response.seq))
            }
            Err(err) => {
                let error_msg = format!("消息发送失败: {}", err);
                error!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    conversation_id = %conversation_id,
                    message_id = %original_message_id,
                    duration_ms = route_duration.as_millis(),
                    "Failed to route message to Message Orchestrator"
                );
                // 返回错误，不再继续处理
                Err(FlareError::message_send_failed(error_msg))
            }
        }
    }

    /// 处理客户端 ACK
    ///
    /// 流程：
    /// 1. 记录日志和指标
    /// 2. 上报 ACK 到 Push Server
    /// 3. 更新会话游标（如果提供）
    #[instrument(skip(self, msg_cmd), fields(connection_id, user_id, message_id = %msg_cmd.message_id))]
    pub async fn handle_client_ack(
        &self,
        connection_id: &str,
        user_id: &str,
        msg_cmd: &MessageCommand,
    ) -> Result<()> {
        let message_id = msg_cmd.message_id.clone();

        info!(
            "✅ 收到客户端ACK: user_id={}, connection_id={}, message_id={}",
            user_id, connection_id, message_id
        );

        // 上报 ACK 到 Push Server
        if let Some(ref ack_publisher) = self.ack_publisher {
            let window_id = msg_cmd
                .metadata
                .get("window_id")
                .and_then(|v| String::from_utf8(v.clone()).ok());
            let ack_seq = msg_cmd
                .metadata
                .get("ack_seq")
                .and_then(|v| std::str::from_utf8(v.as_slice()).ok())
                .and_then(|s| s.parse::<i64>().ok());

            // 创建审计事件
            let ack_event = crate::infrastructure::AckAuditEvent {
                ack: crate::infrastructure::AckData {
                    message_id: message_id.clone(),
                    status: crate::infrastructure::AckStatusValue::Success,
                    error_code: None,
                    error_message: None,
                },
                user_id: user_id.to_string(),
                connection_id: connection_id.to_string(),
                gateway_id: self.gateway_id.clone(),
                timestamp: chrono::Utc::now().timestamp(),
                window_id,
                ack_seq,
            };

            if let Err(e) = ack_publisher.publish_ack(&ack_event).await {
                warn!(
                    ?e,
                    message_id = %message_id,
                    user_id = %user_id,
                    "Failed to publish client ACK"
                );
            }
        }

        // 推送窗口 ACK 更新会话游标（如果提供）
        if let Some(ref conversation_client) = self.conversation_service_client {
            // 从元数据中提取会话信息
            if let Some(conversation_id) = msg_cmd
                .metadata
                .get("conversation_id")
                .and_then(|v| String::from_utf8(v.clone()).ok())
            {
                // 提取时间戳（如果有）
                let message_ts = msg_cmd
                    .metadata
                    .get("timestamp")
                    .and_then(|v| std::str::from_utf8(v.as_slice()).ok())
                    .and_then(|s| s.parse::<i64>().ok())
                    .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());

                // 调用会话服务更新游标
                if let Err(e) = conversation_client
                    .update_session_cursor(user_id, &conversation_id, message_ts)
                    .await
                {
                    warn!(
                        ?e,
                        user_id = %user_id,
                        conversation_id = %conversation_id,
                        "Failed to update session cursor"
                    );
                } else {
                    info!(
                        user_id = %user_id,
                        conversation_id = %conversation_id,
                        message_ts = message_ts,
                        "Session cursor updated successfully"
                    );
                }
            }
        }

        Ok(())
    }

}

