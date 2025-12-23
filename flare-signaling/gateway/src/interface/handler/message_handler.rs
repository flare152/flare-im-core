//! 消息处理模块
//!
//! 实现 Flare 模式的 ServerEventHandler trait，处理客户端消息和连接事件

use async_trait::async_trait;
use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::{Frame, MessageCommand, NotificationCommand, Reliability, generate_message_id, ack_message, frame_with_message_command};
use flare_core::common::protocol::builder::{FrameBuilder, current_timestamp};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::server::events::handler::ServerEventHandler;
use tracing::{debug, error, instrument, warn};

use super::connection::LongConnectionHandler;

/// 实现 ServerEventHandler trait（Flare 模式核心接口）
///
/// Flare 模式会自动路由消息到对应的方法，并自动处理 ACK 响应
#[async_trait]
impl ServerEventHandler for LongConnectionHandler {
    /// 处理 SEND 消息命令
    #[instrument(skip(self), fields(connection_id, message_id = %command.message_id))]
    async fn handle_message(
        &self,
        command: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        let client_message_id = command.message_id.clone();
        // 处理消息发送，获取服务端生成的消息ID
        let server_message_id = self.handle_message_send(command, connection_id).await?;
        
        // 记录服务端消息ID（用于追踪和日志）
        debug!(
            connection_id = %connection_id,
            client_message_id = %command.message_id,
            server_message_id = %server_message_id,
            "Message sent, server_id generated"
        );
        
        // 刷新会话心跳（忽略错误，不影响主流程）
        if let Err(err) = self.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session heartbeat");
        }
        // 创建ack
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("server_message_id".to_string(), server_message_id.as_bytes().to_vec());

        let ack =ack_message(client_message_id, Some(metadata));
        let frame  = frame_with_message_command(ack, Reliability::AtLeastOnce);
        Ok(Some(frame))
    }

    /// 处理 ACK 消息命令
    #[instrument(skip(self), fields(connection_id, message_id = %command.message_id))]
    async fn handle_ack(
        &self,
        command: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        self.handle_client_ack(command, connection_id).await?;
        Ok(None)
    }

    /// 处理 DATA 消息命令（Gateway 暂不支持）
    async fn handle_data(
        &self,
        _command: &MessageCommand,
        _connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        Ok(None)
    }

    /// 处理通知命令（Gateway 暂不支持）
    async fn handle_notification_command(
        &self,
        _command: &NotificationCommand,
        _connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        Ok(None)
    }

    /// 处理连接断开事件
    async fn on_disconnect(&self, connection_id: &str, reason: Option<&str>) -> CoreResult<()> {
        debug!(connection_id = %connection_id, reason = ?reason, "Connection disconnected");
        self.on_disconnect_impl(connection_id).await
    }

    /// 处理连接错误事件
    async fn on_error(&self, connection_id: &str, error: &str) -> CoreResult<()> {
        error!(connection_id = %connection_id, error = %error, "Connection error");
        self.on_disconnect_impl(connection_id).await
    }

    /// 处理 PING 系统命令（框架已自动回复 PONG，这里只处理业务逻辑）
    async fn handle_ping(&self, _frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        let _ = self.refresh_session(connection_id).await;
        Ok(None)
    }

    /// 处理 PONG 系统命令（框架已更新连接活跃时间，这里只处理业务逻辑）
    async fn handle_pong(&self, _frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        let _ = self.refresh_session(connection_id).await;
        Ok(None)
    }

    /// 处理自定义命令
    async fn handle_custom_command(
        &self,
        command: &flare_core::common::protocol::CustomCommand,
        connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        // 构建 Frame 用于处理
        let frame = FrameBuilder::new()
            .with_command(flare_core::common::protocol::flare::core::commands::Command {
                r#type: Some(CommandType::Custom(command.clone())),
            })
            .with_message_id(generate_message_id())
            .with_reliability(Reliability::AtLeastOnce)
            .with_timestamp(current_timestamp())
            .build();
        
        self.handle_frame_impl(&frame, connection_id).await
    }

    /// 处理连接建立完成事件
    async fn on_connect(&self, connection_id: &str) -> CoreResult<()> {
        self.on_connect_impl(connection_id).await
    }

    /// 处理系统事件（Gateway 暂不支持）
    async fn handle_system_event(
        &self,
        _frame: &Frame,
        _connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        Ok(None)
    }
}

// ============================================================================
// 消息处理业务逻辑（协议适配层）
// ============================================================================

impl LongConnectionHandler {
    /// 处理消息发送（协议适配层）
    ///
    /// 从连接信息获取 user_id，委托给应用层服务处理
    ///
    /// # 返回值
    /// 返回服务端生成的消息 ID（server_id），用于在 ACK 中返回给 SDK
    #[instrument(skip(self), fields(connection_id, message_id = %msg_cmd.message_id))]
    pub(crate) async fn handle_message_send(
        &self,
        msg_cmd: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<String> {
        let user_id = self
            .user_id_for_connection(connection_id)
            .await
            .ok_or_else(|| {
                CoreFlareError::system(format!(
                    "user_id is unknown for connection_id={}",
                    connection_id
                ))
            })?;

        let tenant_id = self.get_tenant_id_for_connection(connection_id).await;

        self.message_handler
            .handle_message_send(connection_id, &user_id, msg_cmd, tenant_id.as_deref())
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to handle message send: {}", e)))
    }

    /// 处理客户端 ACK 消息（协议适配层）
    ///
    /// 处理客户端 ACK，更新会话游标，刷新心跳
    #[instrument(skip(self), fields(connection_id, message_id = %msg_cmd.message_id))]
    pub(crate) async fn handle_client_ack(
        &self,
        msg_cmd: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<()> {
        let user_id = self
            .user_id_for_connection(connection_id)
            .await
            .unwrap_or_else(|| "unknown".to_string());

        // 委托给应用层服务处理
        self.message_handler
            .handle_client_ack(connection_id, &user_id, msg_cmd)
            .await?;

        // 推送窗口 ACK 更新会话游标（如果提供）
        if let (Some(conversation_id_bytes), Some(ack_seq_bytes)) = (
            msg_cmd.metadata.get("conversation_id"),
            msg_cmd.metadata.get("ack_seq"),
        ) {
            if let (Ok(conversation_id), Some(ack_seq)) = (
                String::from_utf8(conversation_id_bytes.clone()),
                std::str::from_utf8(ack_seq_bytes.as_slice())
                    .ok()
                    .and_then(|s| s.parse::<i64>().ok()),
            ) {
                if let Ok(mut client) = self.ensure_conversation_client().await {
                    let req = flare_proto::conversation::UpdateCursorRequest {
                        user_id: user_id.clone(),
                        conversation_id,
                        message_ts: ack_seq,
                        tenant: None,
                        device_id: String::new(),
                    };
                    let _ = client.update_cursor(tonic::Request::new(req)).await;
                }
            }
        }

        // 刷新会话心跳（忽略错误，不影响主流程）
        let _ = self.refresh_session(connection_id).await;

        Ok(())
    }

    /// 确保 Conversation 服务客户端已初始化
    ///
    /// 用于更新会话游标等操作
    pub(crate) async fn ensure_conversation_client(
        &self,
    ) -> CoreResult<
        flare_proto::conversation::conversation_service_client::ConversationServiceClient<
            tonic::transport::Channel,
        >,
    > {
        use flare_im_core::service_names::{CONVERSATION, get_service_name};
        use tonic::transport::{Channel, Endpoint};
        let mut guard = self.conversation_service_client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }
        let mut discover_guard = self.conversation_service_discover.lock().await;
        if discover_guard.is_none() {
            let name = get_service_name(CONVERSATION);
            let discover = flare_im_core::discovery::create_discover(&name)
                .await
                .map_err(|e| CoreFlareError::system(format!("create discover: {}", e)))?;
            if let Some(d) = discover {
                *discover_guard = Some(flare_server_core::discovery::ServiceClient::new(d));
            }
        }
        let channel: Channel = if let Some(service_client) = discover_guard.as_mut() {
            match service_client.get_channel().await {
                Ok(ch) => ch,
                Err(_e) => {
                    let addr = std::env::var("CONVERSATION_GRPC_ADDR")
                        .ok()
                        .unwrap_or_else(|| "127.0.0.1:50090".to_string());
                    let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                        .map_err(|err| CoreFlareError::system(err.to_string()))?;
                    endpoint
                        .connect()
                        .await
                        .map_err(|err| CoreFlareError::system(err.to_string()))?
                }
            }
        } else {
            let addr = std::env::var("CONVERSATION_GRPC_ADDR")
                .ok()
                .unwrap_or_else(|| "127.0.0.1:50090".to_string());
            let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            endpoint
                .connect()
                .await
                .map_err(|err| CoreFlareError::system(err.to_string()))?
        };
        let client =
            flare_proto::conversation::conversation_service_client::ConversationServiceClient::new(channel);
        *guard = Some(client.clone());
        Ok(client)
    }
}

