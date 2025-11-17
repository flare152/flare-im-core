//! 连接处理器模块
//!
//! 处理客户端长连接的消息接收和推送

use std::sync::Arc;

use async_trait::async_trait;
use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::{
    Frame, MessageCommand, Reliability, frame_with_message_command, generate_message_id,
};
use flare_core::server::handle::ServerHandle;
use flare_core::server::{ConnectionHandler, ConnectionManagerTrait};
use flare_server_core::error::Result;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::domain::repositories::{SessionStore, SignalingGateway};
use crate::infrastructure::online_cache::OnlineStatusCache;
use crate::infrastructure::messaging::message_router::MessageRouter;
use crate::infrastructure::AckPublisher;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{set_user_id, set_message_id, set_tenant_id};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::instrument;

/// 长连接处理器
///
/// 处理客户端长连接的消息接收和推送
pub struct LongConnectionHandler {
    session_store: Arc<dyn SessionStore>,
    signaling_gateway: Arc<dyn SignalingGateway>,
    online_cache: Arc<OnlineStatusCache>,
    gateway_id: String,
    server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
    manager_trait: Arc<Mutex<Option<Arc<dyn ConnectionManagerTrait>>>>,
    ack_publisher: Option<Arc<dyn AckPublisher>>,
    message_router: Option<Arc<MessageRouter>>,
    metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
}

impl LongConnectionHandler {
    pub fn new(
        session_store: Arc<dyn SessionStore>,
        signaling_gateway: Arc<dyn SignalingGateway>,
        online_cache: Arc<OnlineStatusCache>,
        gateway_id: String,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        message_router: Option<Arc<MessageRouter>>,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    ) -> Self {
        Self {
            session_store,
            signaling_gateway,
            online_cache,
            gateway_id,
            server_handle: Arc::new(Mutex::new(None)),
            manager_trait: Arc::new(Mutex::new(None)),
            ack_publisher,
            message_router,
            metrics,
        }
    }

    /// 设置 ServerHandle
    pub async fn set_server_handle(&self, handle: Arc<dyn ServerHandle>) {
        *self.server_handle.lock().await = Some(handle);
    }

    /// 设置 ConnectionManagerTrait
    pub async fn set_connection_manager(&self, manager: Arc<dyn ConnectionManagerTrait>) {
        *self.manager_trait.lock().await = Some(manager);
    }

    /// 获取用户ID（从连接信息中提取）
    pub async fn user_id_for_connection(&self, connection_id: &str) -> Option<String> {
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                return conn_info.user_id.clone();
            }
        }
        None
    }

    /// 获取连接信息（包括设备ID等）
    async fn get_connection_info(&self, connection_id: &str) -> Option<(String, String)> {
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                let user_id = conn_info.user_id?;
                let device_id = conn_info
                    .device_info
                    .as_ref()
                    .map(|d| d.device_id.clone())
                    .unwrap_or_else(|| "unknown".to_string());
                return Some((user_id, device_id));
            }
        }
        None
    }

    /// 获取连接对应的会话ID
    async fn get_session_id_for_connection(&self, connection_id: &str) -> Option<String> {
        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            // 从会话存储中查找会话
            if let Ok(sessions) = self.session_store.find_by_user(&user_id).await {
                for session in sessions {
                    if session.connection_id.as_deref() == Some(connection_id) {
                        return Some(session.session_id);
                    }
                }
            }
        }
        None
    }

    /// 获取连接对应的租户ID
    async fn get_tenant_id_for_connection(&self, connection_id: &str) -> Option<String> {
        // 从连接信息中提取租户ID（如果连接信息中有）
        // 目前先返回 None，使用默认租户
        None
    }

    /// 注册在线状态到Signaling Online
    async fn register_online_status(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> CoreResult<()> {
        use flare_proto::signaling::LoginRequest;
        use uuid::Uuid;

        let session_id = Uuid::new_v4().to_string();
        let server_id = format!("server-{}", Uuid::new_v4().to_string()[..8].to_string());

        // 构建 metadata，包含 gateway_id（用于跨地区路由）
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("gateway_id".to_string(), self.gateway_id.clone());
        
        let login_request = LoginRequest {
            user_id: user_id.to_string(),
            token: String::new(), // Token 认证暂时为空，后续可以从连接信息中获取
            device_id: device_id.to_string(),
            server_id: server_id.clone(),
            metadata,
            context: None, // RequestContext 暂时为空
            tenant: None, // TenantContext 暂时为空
            device_platform: "unknown".to_string(),
            app_version: "unknown".to_string(),
            desired_conflict_strategy: 0, // 使用默认策略
        };

        match self.signaling_gateway.login(login_request).await {
            Ok(response) => {
                if response.success {
                    // 更新本地缓存
                    self.online_cache
                        .set(
                            user_id.to_string(),
                            self.gateway_id.clone(),
                            true,
                        )
                        .await;

                    info!(
                        user_id = %user_id,
                        gateway_id = %self.gateway_id,
                        session_id = %response.session_id,
                        "Online status registered"
                    );
                } else {
                    warn!(
                        user_id = %user_id,
                        error = %response.error_message,
                        "Failed to register online status"
                    );
                }
            }
            Err(e) => {
                warn!(
                    ?e,
                    user_id = %user_id,
                    "Failed to call signaling login"
                );
                // 即使Signaling失败，也更新本地缓存（降级策略）
                self.online_cache
                    .set(
                        user_id.to_string(),
                        self.gateway_id.clone(),
                        true,
                    )
                    .await;
            }
        }

        Ok(())
    }

    /// 注销在线状态
    async fn unregister_online_status(&self, user_id: &str) -> CoreResult<()> {
        use flare_proto::signaling::LogoutRequest;

        // 先更新本地缓存
        self.online_cache.remove(user_id).await;

        // 查询session_id
        let sessions = self
            .session_store
            .find_by_user(user_id)
            .await
            .map_err(|err| CoreFlareError::system(err.to_string()))?;

        for session in sessions {
            let logout_request = LogoutRequest {
                user_id: user_id.to_string(),
                session_id: session.session_id.clone(),
                context: None, // RequestContext 暂时为空
                tenant: None, // TenantContext 暂时为空
            };

            if let Err(e) = self.signaling_gateway.logout(logout_request).await {
                warn!(
                    ?e,
                    user_id = %user_id,
                    session_id = %session.session_id,
                    "Failed to call signaling logout"
                );
            } else {
                info!(
                    user_id = %user_id,
                    session_id = %session.session_id,
                    "Online status unregistered"
                );
            }
        }

        Ok(())
    }

    /// 主动断开指定连接
    pub async fn disconnect_connection(&self, connection_id: &str) {
        if let Some(handle) = self.server_handle.lock().await.clone() {
            if let Err(err) = handle.disconnect(connection_id).await {
                warn!(?err, %connection_id, "failed to disconnect connection");
            }
        } else {
            warn!(%connection_id, "disconnect requested but server handle not ready");
        }
    }

    /// 刷新连接对应会话的心跳
    pub async fn refresh_session(&self, connection_id: &str) -> Result<()> {
        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            let sessions = self.session_store.find_by_user(&user_id).await?;
            for session in sessions {
                if session.connection_id.as_deref() == Some(connection_id) {
                    let _ = self.session_store.touch(&session.session_id).await?;
                }
            }
        }
        Ok(())
    }

    /// 推送消息到客户端
    pub async fn push_message_to_user(
        &self,
        user_id: &str,
        message: Vec<u8>,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref handle) = *self.server_handle.lock().await {
            let cmd = MessageCommand {
                r#type: 0,
                message_id: generate_message_id(),
                payload: message,
                metadata: Default::default(),
                seq: 0,
            };

            let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

            handle
                .send_to_user(user_id, &frame)
                .await
                .map_err(|e| format!("Failed to send message: {}", e))?;

            info!("Pushed message to user {}", user_id);
        } else {
            return Err("ServerHandle not initialized".into());
        }

        Ok(())
    }

    /// 推送消息到指定连接
    pub async fn push_message_to_connection(
        &self,
        connection_id: &str,
        message: Vec<u8>,
    ) -> std::result::Result<(), String> {
        if let Some(ref handle) = *self.server_handle.lock().await {
            let cmd = MessageCommand {
                r#type: 0,
                message_id: generate_message_id(),
                payload: message,
                metadata: Default::default(),
                seq: 0,
            };

            let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

            handle
                .send_to(connection_id, &frame)
                .await
                .map_err(|e| format!("Failed to send message: {}", e))?;

            debug!("Pushed message to connection {}", connection_id);
        } else {
            return Err("ServerHandle not initialized".to_string());
        }

        Ok(())
    }
}

#[async_trait]
impl ConnectionHandler for LongConnectionHandler {
    async fn handle_frame(&self, frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        debug!(
            "Received frame from connection {}: {:?}",
            connection_id, frame
        );

        if let Some(cmd) = &frame.command {
            if let Some(CommandType::Message(msg_cmd)) = &cmd.r#type {
                let message_type = msg_cmd.r#type;

                // 处理客户端ACK消息（Type::Ack = 1）
                if message_type == 1 {
                    // 这是客户端ACK消息
                    let user_id = self
                        .user_id_for_connection(connection_id)
                        .await
                        .unwrap_or_else(|| "unknown".to_string());

                    let message_id = msg_cmd.message_id.clone();

                    info!(
                        "✅ 收到客户端ACK: user_id={}, connection_id={}, message_id={}",
                        user_id,
                        connection_id,
                        message_id
                    );

                    // 设置追踪属性
                    #[cfg(feature = "tracing")]
                    {
                        let span = Span::current();
                        set_user_id(&span, &user_id);
                        set_message_id(&span, &message_id);
                        span.record("ack_type", "client_ack");
                    }

                    // 记录客户端ACK指标
                    // 注意：这里无法获取 tenant_id，使用 "unknown"
                    self.metrics.client_ack_received_total
                        .with_label_values(&["unknown"])
                        .inc();

                    // 上报推送ACK到Kafka
                    if let Some(ref ack_publisher) = self.ack_publisher {
                        let ack_event = crate::infrastructure::PushAckEvent {
                            message_id: message_id.clone(),
                            user_id: user_id.clone(),
                            connection_id: connection_id.to_string(),
                            gateway_id: self.gateway_id.clone(),
                            ack_type: "client_ack".to_string(),
                            status: "success".to_string(),
                            timestamp: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64,
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

                    // 刷新会话心跳
                    if let Err(err) = self.refresh_session(connection_id).await {
                        warn!(?err, %connection_id, "failed to refresh session heartbeat");
                    }

                    return Ok(None);
                }

                // 处理普通消息（Type::Send = 0）
                if message_type == 0 {
                    let user_id = self
                        .user_id_for_connection(connection_id)
                        .await
                        .unwrap_or_else(|| "unknown".to_string());

                    info!(
                        "📨 收到消息: user_id={}, connection_id={}, message_len={}",
                        user_id,
                        connection_id,
                        msg_cmd.payload.len()
                    );

                    // 路由消息到 Message Orchestrator
                    if let Some(ref router) = self.message_router {
                        // 获取会话ID（从连接信息中提取，或使用默认聊天室会话）
                        let session_id = self
                            .get_session_id_for_connection(connection_id)
                            .await
                            .unwrap_or_else(|| format!("chatroom:{}", self.gateway_id));

                        // 获取租户ID（从连接信息中提取，或使用默认）
                        let tenant_id = self
                            .get_tenant_id_for_connection(connection_id)
                            .await;

                        // 路由消息
                        match router
                            .route_message(
                                &user_id,
                                &session_id,
                                msg_cmd.payload.clone(),
                                tenant_id.as_deref(),
                            )
                            .await
                        {
                            Ok(response) => {
                                info!(
                                    user_id = %user_id,
                                    session_id = %session_id,
                                    message_id = %response.message_id,
                                    "Message routed successfully"
                                );
                            }
                            Err(err) => {
                                tracing::error!(
                                    ?err,
                                    user_id = %user_id,
                                    session_id = %session_id,
                                    "Failed to route message to Message Orchestrator"
                                );
                                // 即使路由失败，也刷新会话心跳
                            }
                        }
                    } else {
                        warn!("Message Router not configured, message will not be routed");
                    }

                    if let Err(err) = self.refresh_session(connection_id).await {
                        warn!(?err, %connection_id, "failed to refresh session heartbeat");
                    }
                }
            }
        }

        Ok(None)
    }

    #[instrument(skip(self), fields(connection_id))]
    async fn on_connect(&self, connection_id: &str) -> CoreResult<()> {
        info!("✅ 新连接: {}", connection_id);
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 更新活跃连接数
        if let Some(ref handle) = *self.server_handle.lock().await {
            let count = handle.connection_count();
            self.metrics.connections_active.set(count as i64);
        }

        if let Some((user_id, device_id)) = self.get_connection_info(connection_id).await {
            // 1. 更新session连接信息
            let sessions = self
                .session_store
                .find_by_user(&user_id)
                .await
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            for session in sessions {
                self.session_store
                    .update_connection(&session.session_id, Some(connection_id.to_string()))
                    .await
                    .map_err(|err| CoreFlareError::system(err.to_string()))?;
            }

            // 2. 注册在线状态到Signaling Online
            if let Err(err) = self.register_online_status(&user_id, &device_id).await {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to register online status"
                );
            }

            info!(
                "📝 用户已连接: user_id={}, connection_id={}, device_id={}",
                user_id, connection_id, device_id
            );
        }

        Ok(())
    }

    #[instrument(skip(self), fields(connection_id))]
    async fn on_disconnect(&self, connection_id: &str) -> CoreResult<()> {
        info!("❌ 连接断开: {}", connection_id);
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 记录连接断开指标
        self.metrics.connection_disconnected_total.inc();

        // 更新活跃连接数
        if let Some(ref handle) = *self.server_handle.lock().await {
            let count = handle.connection_count();
            self.metrics.connections_active.set(count as i64);
        }

        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            // 1. 更新session连接信息
            let sessions = self
                .session_store
                .find_by_user(&user_id)
                .await
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            
            // 检查是否还有其他连接
            let mut has_other_connections = false;
            for session in &sessions {
                if let Some(ref conn_id) = session.connection_id {
                    if conn_id != connection_id {
                        has_other_connections = true;
                        break;
                    }
                }
            }

            // 如果没有其他连接，注销在线状态
            if !has_other_connections {
                if let Err(err) = self.unregister_online_status(&user_id).await {
                    warn!(
                        ?err,
                        user_id = %user_id,
                        connection_id = %connection_id,
                        "Failed to unregister online status"
                    );
                }
            }

            // 更新session连接信息
            for session in sessions {
                // 如果这个session的连接ID匹配，清除连接ID
                if session.connection_id.as_deref() == Some(connection_id) {
                    self.session_store
                        .update_connection(&session.session_id, None)
                        .await
                        .map_err(|err| CoreFlareError::system(err.to_string()))?;
                }
            }

            info!(
                "📝 用户已断开: user_id={}, connection_id={}",
                user_id, connection_id
            );
        }

        Ok(())
    }
}
