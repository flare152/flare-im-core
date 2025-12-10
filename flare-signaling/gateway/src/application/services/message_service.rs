//! 消息处理应用服务
//!
//! 处理消息收发的业务流程编排

use std::sync::Arc;
use flare_core::common::error::{Result, FlareError};
use flare_core::common::protocol::MessageCommand;
use tracing::{info, warn, error, instrument};
use prost::Message as ProstMessage;

use crate::infrastructure::messaging::message_router::MessageRouter;
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::infrastructure::AckPublisher;
use crate::domain::service::SessionDomainService;

/// 消息处理应用服务
///
/// 职责：
/// - 编排消息接收流程
/// - 编排消息发送流程
/// - 编排 ACK 处理流程
pub struct MessageApplicationService {
    message_router: Option<Arc<MessageRouter>>,
    ack_sender: Arc<AckSender>,
    ack_publisher: Option<Arc<dyn AckPublisher>>,
    session_domain_service: Arc<SessionDomainService>,
    gateway_id: String,
}

impl MessageApplicationService {
    pub fn new(
        message_router: Option<Arc<MessageRouter>>,
        ack_sender: Arc<AckSender>,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        session_domain_service: Arc<SessionDomainService>,
        gateway_id: String,
    ) -> Self {
        Self {
            message_router,
            ack_sender,
            ack_publisher,
            session_domain_service,
            gateway_id,
        }
    }

    /// 处理消息发送
    ///
    /// 流程：
    /// 1. 从 payload 中提取 session_id
    /// 2. 路由消息到 Message Orchestrator
    /// 3. 发送 ACK 到客户端
    #[instrument(skip(self, msg_cmd), fields(connection_id, user_id, message_id = %msg_cmd.message_id))]
    pub async fn handle_message_send(
        &self,
        connection_id: &str,
        user_id: &str,
        msg_cmd: &MessageCommand,
        tenant_id: Option<&str>,
    ) -> Result<()> {
        info!(
            user_id = %user_id,
            connection_id = %connection_id,
            message_len = msg_cmd.payload.len(),
            "Message received"
        );

        // 检查消息路由器
        let router = match &self.message_router {
            Some(router) => router,
            None => {
                warn!("Message Router not configured, message will not be routed");
                return Ok(());
            }
        };

        // 从 payload 中提取 session_id
        let session_id = match self.extract_session_id_from_payload(&msg_cmd.payload) {
            Ok(sid) => sid,
            Err(e) => {
                let error_msg = format!("无效的消息格式: {}", e);
                error!(
                    ?e,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to extract session_id from message payload"
                );
                // 发送错误通知
                self.ack_sender.send_error_notification(
                    connection_id,
                    &msg_cmd.message_id,
                    &error_msg,
                ).await?;
                return Ok(());
            }
        };

        // 路由消息
        let original_message_id = msg_cmd.message_id.clone();
        let route_res = router.route_message(
            user_id,
            &session_id,
            msg_cmd.payload.clone(),
            tenant_id,
        ).await;

        match route_res {
            Ok(response) => {
                info!(
                    user_id = %user_id,
                    session_id = %session_id,
                    message_id = %response.message_id,
                    "Message routed successfully"
                );
                // 发送 ACK 到客户端
                self.ack_sender.send_message_ack(
                    connection_id,
                    &response.message_id,
                    &session_id,
                ).await?;
            }
            Err(err) => {
                let error_msg = format!("消息发送失败: {}", err);
                error!(
                    ?err,
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to route message to Message Orchestrator"
                );
                // 发送错误通知
                self.ack_sender.send_error_notification(
                    connection_id,
                    &original_message_id,
                    &error_msg,
                ).await?;
            }
        }

        Ok(())
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
            let window_id = msg_cmd.metadata.get("window_id")
                .and_then(|v| String::from_utf8(v.clone()).ok());
            let ack_seq = msg_cmd.metadata.get("ack_seq")
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
        // 注意：这里需要调用 Session Service，暂时保留原有逻辑
        // TODO: 将 Session Service 调用移到领域层或应用层独立服务

        Ok(())
    }

    /// 从 payload 中提取 session_id
    fn extract_session_id_from_payload(&self, payload: &[u8]) -> Result<String> {
        use flare_proto::Message as ProtoMessage;

        let message = ProtoMessage::decode(payload)
            .map_err(|e| FlareError::deserialization_error(
                format!("Failed to decode Message from payload: {}", e)
            ))?;

        if message.session_id.is_empty() {
            return Err(FlareError::message_format_error(
                "Message.session_id is required but empty".to_string()
            ));
        }

        Ok(message.session_id)
    }
}
