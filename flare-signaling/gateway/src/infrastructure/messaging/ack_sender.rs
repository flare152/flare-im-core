//! ACK 发送器
//!
//! 负责向客户端发送 ACK 和错误通知
//!
//! Gateway 基础设施层职责：封装 ACK 发送的底层实现

use flare_core::common::error::{FlareError, Result};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::flare::core::commands::notification_command::Type as NotificationType;
use flare_core::common::protocol::{
    Frame, MessageCommand, Reliability, builder::FrameBuilder, frame_with_notification_command,
    notification,
};
use flare_core::server::handle::ServerHandle;
use prost::Message as ProstMessage;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn};

/// ACK 发送器
///
/// 提供向客户端发送 ACK 和错误通知的功能
pub struct AckSender {
    server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
}

impl AckSender {
    /// 创建新的 ACK 发送器
    pub fn new(server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>) -> Self {
        Self { server_handle }
    }

    /// 发送消息 ACK 到客户端
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `message_id` - 服务端生成的消息ID（server_id），来自 SendMessageResponse.message_id
    /// * `conversation_id` - 会话 ID
    ///
    /// # ACK 类型
    /// 根据 MessageCommand 规范和 transport.proto 定义：
    /// - Type::Ack (1)：确认回执
    /// - payload：序列化的 SendEnvelopeAck（来自 transport.proto）
    ///   - SendEnvelopeAck.message_id 字段包含服务端生成的消息ID（server_id）
    /// - metadata 包含：conversation_id（用于路由）
    ///
    /// # 说明
    /// SDK 会从 ACK 的 SendEnvelopeAck.message_id 字段获取服务端消息ID，
    /// 并更新本地消息的 server_id 字段，完成 client_msg_id 到 server_id 的映射
    pub async fn send_message_ack(
        &self,
        connection_id: &str,
        message_id: &str,
        conversation_id: &str,
    ) -> Result<()> {
        // 构建 SendEnvelopeAck（使用 transport.proto 定义）
        // message_id 字段包含服务端生成的消息ID（server_id），SDK 会使用此ID更新本地消息
        let send_ack = flare_proto::common::SendEnvelopeAck {
            server_msg_id: message_id.to_string(), // 服务端生成的消息ID（server_id）
            status: flare_proto::common::AckStatus::Success as i32,
            seq: 0, // 消息序列号（可选）
            error_code: 0,
            error_message: String::new(),
        };

        // 序列化为 protobuf bytes
        let mut payload = Vec::new();
        send_ack.encode(&mut payload).map_err(|e| {
            FlareError::serialization_error(format!("Failed to encode SendEnvelopeAck: {}", e))
        })?;

        // 构建 ACK metadata（只保留路由必需的 conversation_id）
        let mut md = std::collections::HashMap::new();
        md.insert("conversation_id".to_string(), conversation_id.as_bytes().to_vec());

        // 创建 ACK 命令（Type::Ack = 1）
        let ack_cmd = MessageCommand {
            r#type: flare_core::common::protocol::flare::core::commands::message_command::Type::Ack
                as i32,
            message_id: message_id.to_string(),
            payload, // 使用 SendEnvelopeAck 作为 payload
            metadata: md,
            seq: 0,
        };

        let command = flare_core::common::protocol::flare::core::commands::Command {
            r#type: Some(CommandType::Message(ack_cmd)),
        };

        let ack_frame = FrameBuilder::new()
            .with_command(command)
            .with_message_id(message_id.to_string())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        // 发送 ACK
        self.send_frame(connection_id, &ack_frame).await?;

        info!(
            connection_id = %connection_id,
            message_id = %message_id,
            conversation_id = %conversation_id,
            "Message ACK sent to client (SendEnvelopeAck)"
        );

        Ok(())
    }

    /// 发送失败 ACK 到客户端
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `message_id` - 消息 ID
    /// * `conversation_id` - 会话 ID
    /// * `error_code` - 错误码
    /// * `error_message` - 错误信息
    ///
    /// # 使用场景
    /// 当消息路由失败、验证失败等情况时，向客户端发送失败 ACK
    pub async fn send_message_ack_failed(
        &self,
        connection_id: &str,
        message_id: &str,
        conversation_id: &str,
        error_code: i32,
        error_message: String,
    ) -> Result<()> {
        // 构建失败的 SendEnvelopeAck
        let send_ack = flare_proto::common::SendEnvelopeAck {
            server_msg_id: message_id.to_string(),
            status: flare_proto::common::AckStatus::Failed as i32,
            seq: 0,
            error_code,
            error_message,
        };

        // 序列化为 protobuf bytes
        let mut payload = Vec::new();
        send_ack.encode(&mut payload).map_err(|e| {
            FlareError::serialization_error(format!("Failed to encode SendEnvelopeAck: {}", e))
        })?;

        // 构建 ACK metadata
        let mut md = std::collections::HashMap::new();
        md.insert("conversation_id".to_string(), conversation_id.as_bytes().to_vec());

        // 创建 ACK 命令
        let ack_cmd = MessageCommand {
            r#type: flare_core::common::protocol::flare::core::commands::message_command::Type::Ack
                as i32,
            message_id: message_id.to_string(),
            payload,
            metadata: md,
            seq: 0,
        };

        let command = flare_core::common::protocol::flare::core::commands::Command {
            r#type: Some(CommandType::Message(ack_cmd)),
        };

        let ack_frame = FrameBuilder::new()
            .with_command(command)
            .with_message_id(message_id.to_string())
            .with_reliability(Reliability::AtLeastOnce)
            .build();

        // 发送失败 ACK
        self.send_frame(connection_id, &ack_frame).await?;

        warn!(
            connection_id = %connection_id,
            message_id = %message_id,
            conversation_id = %conversation_id,
            error_code = error_code,
            "Failed ACK sent to client (SendEnvelopeAck)"
        );

        Ok(())
    }

    /// 发送错误通知到客户端
    ///
    /// # 参数
    /// * `connection_id` - 连接 ID
    /// * `original_message_id` - 原始消息 ID（可选）
    /// * `error_msg` - 错误信息
    ///
    /// # 通知类型
    /// 使用 NotificationType::Alert 发送错误通知
    pub async fn send_error_notification(
        &self,
        connection_id: &str,
        original_message_id: &str,
        error_msg: &str,
    ) -> Result<()> {
        // 构建错误通知 metadata
        let mut metadata = std::collections::HashMap::new();
        if !original_message_id.is_empty() {
            metadata.insert(
                "original_message_id".to_string(),
                original_message_id.as_bytes().to_vec(),
            );
        }
        metadata.insert(
            "error_code".to_string(),
            "ROUTING_FAILED".as_bytes().to_vec(),
        );

        // 创建错误通知
        let error_notification = notification(
            NotificationType::Alert,
            "消息发送失败".to_string(),
            error_msg.as_bytes().to_vec(),
            Some(metadata),
        );

        let error_frame =
            frame_with_notification_command(error_notification, Reliability::AtLeastOnce);

        // 发送错误通知
        if let Err(send_err) = self.send_frame(connection_id, &error_frame).await {
            warn!(
                ?send_err,
                connection_id = %connection_id,
                "Failed to send error notification to client"
            );
        } else {
            info!(
                connection_id = %connection_id,
                "Error notification sent to client"
            );
        }

        Ok(())
    }

    /// 发送 Frame 到指定连接（内部辅助方法）
    async fn send_frame(&self, connection_id: &str, frame: &Frame) -> Result<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(FlareError::system(
                    "ServerHandle not initialized".to_string(),
                ));
            }
        };

        handle
            .send_to(connection_id, frame)
            .await
            .map_err(|e| FlareError::system(format!("Failed to send frame: {}", e)))?;

        Ok(())
    }
}
