//! 连接处理器模块
//!
//! 处理客户端长连接的消息接收和推送

use std::sync::Arc;

use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::{
    Frame, MessageCommand, Reliability, frame_with_message_command, generate_message_id,
};
use flare_core::server::handle::ServerHandle;
use flare_core::server::{ConnectionHandler, ConnectionManagerTrait};
use async_trait::async_trait;
use flare_core::server::builder::flare::MessageListener;
use flare_server_core::discovery::ServiceClient;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::domain::repository::{SessionStore, SignalingGateway};
use crate::infrastructure::online_cache::OnlineStatusCache;
use crate::infrastructure::messaging::message_router::MessageRouter;
use crate::infrastructure::AckPublisher;
use crate::config::AccessGatewayConfig;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{set_user_id, set_message_id, set_tenant_id};
use chrono::Utc;
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
    session_service_client: Arc<Mutex<Option<flare_proto::session::session_service_client::SessionServiceClient<tonic::transport::Channel>>>>,
    session_service_discover: Arc<Mutex<Option<ServiceClient>>>,
    config: Arc<AccessGatewayConfig>,
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
        config: Arc<AccessGatewayConfig>,
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
            session_service_client: Arc::new(Mutex::new(None)),
            session_service_discover: Arc::new(Mutex::new(None)),
            config,
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
    /// 
    /// 优化：使用辅助函数查找会话，减少代码重复
    async fn get_session_id_for_connection(&self, connection_id: &str) -> Option<String> {
        self.find_session_by_connection(connection_id).await
            .map(|session| session.session_id)
    }

    /// 根据连接ID查找会话（内部辅助函数）
    /// 
    /// 提取会话查找逻辑，减少代码重复
    async fn find_session_by_connection(&self, connection_id: &str) -> Option<crate::domain::model::Session> {
        let user_id = self.user_id_for_connection(connection_id).await?;
        
        // 从会话存储中查找会话
        let sessions = self.session_store.find_by_user(&user_id).await.ok()?;
        
        // 查找匹配的连接ID的会话
        sessions.into_iter()
            .find(|session| session.connection_id.as_deref() == Some(connection_id))
    }

    /// 获取连接对应的租户ID
    async fn get_tenant_id_for_connection(&self, _connection_id: &str) -> Option<String> {
        // 从连接信息中提取租户ID（如果连接信息中有）
        // 目前先返回 None，使用默认租户
        None
    }

    /// 注册在线状态到Signaling Online
    async fn register_online_status(
        &self,
        user_id: &str,
        device_id: &str,
        connection_id: Option<&str>,
    ) -> CoreResult<()> {
        use flare_proto::signaling::LoginRequest;
        use uuid::Uuid;

        let _session_id = Uuid::new_v4().to_string();
        // 使用 gateway_id 作为 server_id，这样 Signaling Online 可以直接返回 gateway_id
        let server_id = self.gateway_id.clone();

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

        // 为 signaling_gateway.login 添加超时（5秒），防止阻塞
        let login_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.signaling_gateway.login(login_request)
        ).await;

        match login_result {
            Ok(Ok(response)) => {
                if response.success {
                    // 创建并存储会话信息到 Redis
                    use crate::domain::model::Session;
                    let session = Session::new(
                        response.session_id.clone(),
                        user_id.to_string(),
                        device_id.to_string(),
                        Some(response.route_server.clone()),
                        self.gateway_id.clone(),
                    );
                    
                    // 存储会话到 Redis（这样 Push Server 才能查询到在线用户）
                    if let Err(err) = self.session_store.insert(session.clone()).await {
                        warn!(
                            ?err,
                            user_id = %user_id,
                            session_id = %response.session_id,
                            "Failed to store session in Redis"
                        );
                    } else {
                        info!(
                            user_id = %user_id,
                            session_id = %response.session_id,
                            "Session stored in Redis"
                        );
                    }
                    
                    // 更新会话的连接信息（如果连接已建立）
                    if let Some(conn_id) = connection_id {
                        if let Err(err) = self.session_store.update_connection(&response.session_id, Some(conn_id.to_string())).await {
                            warn!(
                                ?err,
                                user_id = %user_id,
                                session_id = %response.session_id,
                                connection_id = %conn_id,
                                "Failed to update session connection"
                            );
                        }
                    }
                    
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
            Ok(Err(e)) => {
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
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    "Timeout while calling signaling login (5s)"
                );
                // 即使Signaling超时，也更新本地缓存（降级策略）
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
    pub async fn refresh_session(&self, connection_id: &str) -> CoreResult<()> {
        // 查找会话
        let session = match self.find_session_by_connection(connection_id).await {
            Some(session) => session,
            None => {
                // 会话不存在，可能是连接还未完全建立，不记录错误
                return Ok(());
            }
        };

        // 1. 更新 Redis 中的会话信息
        if let Err(e) = self.session_store.touch(&session.session_id).await {
            warn!(?e, session_id = %session.session_id, "Failed to touch session");
        }
        
        // 2. 调用 Signaling Online 服务的心跳接口，更新在线状态
        self.send_heartbeat_to_signaling(&session.user_id, &session.session_id, connection_id).await;
        
        Ok(())
    }

    /// 发送心跳到 Signaling Online 服务（内部辅助函数）
    /// 
    /// 提取心跳发送逻辑，减少代码重复
    async fn send_heartbeat_to_signaling(
        &self,
        user_id: &str,
        session_id: &str,
        connection_id: &str,
    ) {
        use flare_proto::signaling::HeartbeatRequest;
        
        let heartbeat_request = HeartbeatRequest {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            context: None,
            tenant: None,
        };
        
        // 添加超时保护，避免阻塞
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.signaling_gateway.heartbeat(heartbeat_request)
        ).await {
            Ok(Ok(_)) => {
                debug!(
                    user_id = %user_id,
                    session_id = %session_id,
                    connection_id = %connection_id,
                    "Heartbeat sent to Signaling Online service"
                );
            },
            Ok(Err(e)) => {
                warn!(
                    error = %e,
                    user_id = %user_id,
                    session_id = %session_id,
                    connection_id = %connection_id,
                    "Failed to send heartbeat to Signaling Online service"
                );
            },
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    session_id = %session_id,
                    connection_id = %connection_id,
                    "Timeout sending heartbeat to Signaling Online service (3s)"
                );
            }
        }
    }

    /// 推送消息到客户端
    pub async fn push_message_to_user(
        &self,
        user_id: &str,
        message: Vec<u8>,
    ) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system("ServerHandle not initialized".to_string()));
            }
        };

        let cmd = MessageCommand {
            r#type: 0,
            message_id: generate_message_id(),
            payload: message,
            metadata: Default::default(),
            seq: 0,
        };

        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

        handle.send_to_user(user_id, &frame).await
            .map_err(|e| CoreFlareError::system(format!("Failed to send message: {}", e)))?;

        info!(
            user_id = %user_id,
            "Message pushed to user"
        );
        Ok(())
    }

    /// 推送消息到指定连接
    pub async fn push_message_to_connection(
        &self,
        connection_id: &str,
        message: Vec<u8>,
    ) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system("ServerHandle not initialized".to_string()));
            }
        };

        let cmd = MessageCommand {
            r#type: 0,
            message_id: generate_message_id(),
            payload: message,
            metadata: Default::default(),
            seq: 0,
        };

        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

        handle.send_to(connection_id, &frame).await
            .map_err(|e| CoreFlareError::system(format!("Failed to send message: {}", e)))?;

        debug!(
            connection_id = %connection_id,
            "Message pushed to connection"
        );
        Ok(())
    }
}

// 实现 MessageListener（用于 FlareServerBuilder）

#[async_trait]
impl MessageListener for LongConnectionHandler {
    async fn on_message(&self, frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        self.handle_frame_impl(frame, connection_id).await
    }
    
    async fn on_connect(&self, connection_id: &str) -> CoreResult<()> {
        self.on_connect_impl(connection_id).await
    }
    
    async fn on_disconnect(&self, connection_id: &str, reason: Option<&str>) -> CoreResult<()> {
        self.on_disconnect_impl(connection_id).await
    }
}

// 保留 ConnectionHandler 实现以兼容

#[async_trait]
impl ConnectionHandler for LongConnectionHandler {
    async fn handle_frame(&self, frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        self.handle_frame_impl(frame, connection_id).await
    }
    
    async fn on_connect(&self, connection_id: &str) -> CoreResult<()> {
        self.on_connect_impl(connection_id).await
    }
    
    async fn on_disconnect(&self, connection_id: &str) -> CoreResult<()> {
        self.on_disconnect_impl(connection_id).await
    }
}

impl LongConnectionHandler {
    /// 处理消息帧的内部实现
    async fn handle_frame_impl(&self, frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        debug!(
            "Received frame from connection {}: {:?}",
            connection_id, frame
        );

        if let Some(cmd) = &frame.command {
            if let Some(CommandType::Message(msg_cmd)) = &cmd.r#type {
                let message_type = msg_cmd.r#type;

                // 处理客户端ACK消息（Type::Ack = 1）
                if message_type == 1 {
                    self.handle_client_ack(msg_cmd, connection_id).await?;
                    return Ok(None);
                }

                // 处理普通消息（Type::Send = 0）
                if message_type == 0 {
                    self.handle_message_send(frame, msg_cmd, connection_id).await?;
                    if let Err(err) = self.refresh_session(connection_id).await {
                        warn!(?err, %connection_id, "failed to refresh session heartbeat");
                    }
                }
            }

            if let Some(CommandType::Custom(custom_cmd)) = &cmd.r#type {
                let request_id = frame
                    .metadata
                    .get("request_id")
                    .and_then(|v| String::from_utf8(v.clone()).ok())
                    .unwrap_or_else(|| frame.message_id.clone());

                match custom_cmd.name.as_str() {
                    "SessionBootstrap" => {
                        use flare_proto::session::{SessionBootstrapRequest, SessionBootstrapResponse};
                        use prost::Message as _;
                        let req = SessionBootstrapRequest::decode(&custom_cmd.data[..])
                            .map_err(|e| CoreFlareError::deserialization_error(format!("decode SessionBootstrapRequest: {}", e)))?;
                        let mut client = self.ensure_session_client().await?;
                        let resp = client.session_bootstrap(req).await
                            .map_err(|status| CoreFlareError::system(status.to_string()))?
                            .into_inner();
                        let mut buf = Vec::new();
                        SessionBootstrapResponse::encode(&resp, &mut buf);
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                        let response_frame = flare_core::common::protocol::builder::FrameBuilder::new()
                            .with_command(flare_core::common::protocol::flare::core::commands::Command { r#type: Some(CommandType::Custom(flare_core::common::protocol::CustomCommand { name: "SessionBootstrap".to_string(), data: buf, metadata })) })
                            .with_message_id(request_id)
                            .with_reliability(Reliability::AtLeastOnce)
                            .build();
                        return Ok(Some(response_frame));
                    }
                    "SyncMessages" => {
                        use flare_proto::session::{SyncMessagesRequest, SyncMessagesResponse};
                        use prost::Message as _;
                        let req = SyncMessagesRequest::decode(&custom_cmd.data[..])
                            .map_err(|e| CoreFlareError::deserialization_error(format!("decode SyncMessagesRequest: {}", e)))?;
                        let mut client = self.ensure_session_client().await?;
                        let resp = client.sync_messages(req).await
                            .map_err(|status| CoreFlareError::system(status.to_string()))?
                            .into_inner();
                        let mut buf = Vec::new();
                        SyncMessagesResponse::encode(&resp, &mut buf);
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                        let response_frame = flare_core::common::protocol::builder::FrameBuilder::new()
                            .with_command(flare_core::common::protocol::flare::core::commands::Command { r#type: Some(CommandType::Custom(flare_core::common::protocol::CustomCommand { name: "SyncMessages".to_string(), data: buf, metadata })) })
                            .with_message_id(request_id)
                            .with_reliability(Reliability::AtLeastOnce)
                            .build();
                        return Ok(Some(response_frame));
                    }
                    "ListSessions" => {
                        use flare_proto::session::{ListSessionsRequest, ListSessionsResponse};
                        use prost::Message as _;
                        let req = ListSessionsRequest::decode(&custom_cmd.data[..])
                            .map_err(|e| CoreFlareError::deserialization_error(format!("decode ListSessionsRequest: {}", e)))?;
                        let mut client = self.ensure_session_client().await?;
                        let resp = client.list_sessions(req).await
                            .map_err(|status| CoreFlareError::system(status.to_string()))?
                            .into_inner();
                        let mut buf = Vec::new();
                        ListSessionsResponse::encode(&resp, &mut buf);
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
                        let response_frame = flare_core::common::protocol::builder::FrameBuilder::new()
                            .with_command(flare_core::common::protocol::flare::core::commands::Command { r#type: Some(CommandType::Custom(flare_core::common::protocol::CustomCommand { name: "ListSessions".to_string(), data: buf, metadata })) })
                            .with_message_id(request_id)
                            .with_reliability(Reliability::AtLeastOnce)
                            .build();
                        return Ok(Some(response_frame));
                    }
                    _ => {}
                }
            }

            if let Some(CommandType::System(sys_cmd)) = &cmd.r#type {
                // 仅处理 System::Event（业务事件）
                use flare_core::common::protocol::flare::core::commands::system_command::Type as SysType;
                if sys_cmd.r#type == SysType::Event as i32 {
                    let user_id = self
                        .user_id_for_connection(connection_id)
                        .await
                        .unwrap_or_else(|| "unknown".to_string());

                    let tenant_id = self.get_tenant_id_for_connection(connection_id).await;

                    // 事件名：edit / reaction_add / reaction_remove / read / recall
                    let event_name = sys_cmd.message.as_str();
                    match event_name {
                        "reaction_add" => {
                            let emoji = sys_cmd
                                .metadata
                                .get("emoji")
                                .and_then(|b| String::from_utf8(b.clone()).ok())
                                .unwrap_or_default();
                            let message_id = String::from_utf8(sys_cmd.data.clone()).unwrap_or_default();
                            if let Some(ref router) = self.message_router {
                                let _ = router
                                    .route_add_reaction(&message_id, &emoji, tenant_id.as_deref(), &user_id)
                                    .await;
                            }
                        }
                        "reaction_remove" => {
                            let emoji = sys_cmd
                                .metadata
                                .get("emoji")
                                .and_then(|b| String::from_utf8(b.clone()).ok())
                                .unwrap_or_default();
                            let message_id = String::from_utf8(sys_cmd.data.clone()).unwrap_or_default();
                            if let Some(ref router) = self.message_router {
                                let _ = router
                                    .route_remove_reaction(&message_id, &emoji, tenant_id.as_deref(), &user_id)
                                    .await;
                            }
                        }
                        "edit" => {
                            // data 携带完整 Message（SDK侧加密，网关不解密，当前按属性更新）
                            // 如果需要按 content 更新，需要服务端实现对应更新接口
                            let mut attributes = std::collections::HashMap::new();
                            // 尝试将 data 当作UTF8解析为 JSON 属性对，失败则为空
                            if let Ok(raw) = String::from_utf8(sys_cmd.data.clone()) {
                                // 允许 data 传输形如 key1=value1;key2=value2 的简易格式
                                for part in raw.split(';') {
                                    if let Some((k, v)) = part.split_once('=') {
                                        attributes.insert(k.to_string(), v.to_string());
                                    }
                                }
                            }
                            // message_id 从 metadata 或 data 中获取优先
                            let message_id = sys_cmd
                                .metadata
                                .get("message_id")
                                .and_then(|b| String::from_utf8(b.clone()).ok())
                                .unwrap_or_else(|| String::from_utf8(sys_cmd.data.clone()).unwrap_or_default());
                            if let Some(ref router) = self.message_router {
                                let _ = router
                                    .route_edit_message(&message_id, attributes, tenant_id.as_deref(), &user_id)
                                    .await;
                            }
                        }
                        "read" => {
                            // 从 metadata 或 data 获取 message_id
                            let message_id = sys_cmd
                                .metadata
                                .get("message_id")
                                .and_then(|b| String::from_utf8(b.clone()).ok())
                                .unwrap_or_else(|| String::from_utf8(sys_cmd.data.clone()).unwrap_or_default());
                            if let Some(ref router) = self.message_router {
                                let _ = router
                                    .route_mark_read(&message_id, tenant_id.as_deref(), &user_id)
                                    .await;
                            }
                        }
                        "recall" => {
                            let message_id = String::from_utf8(sys_cmd.data.clone()).unwrap_or_default();
                            if let Some(ref router) = self.message_router {
                                let _ = router
                                    .route_recall_message(&message_id, tenant_id.as_deref(), &user_id)
                                    .await;
                            }
                        }
                        _ => {
                            // 未知事件，忽略
                        }
                    }

                    // 刷新会话心跳
                    if let Err(err) = self.refresh_session(connection_id).await {
                        warn!(?err, %connection_id, "failed to refresh session heartbeat");
                    }
                }
            }
        }

        Ok(None)
    }
}

impl LongConnectionHandler {
    async fn ensure_session_client(&self) -> CoreResult<flare_proto::session::session_service_client::SessionServiceClient<tonic::transport::Channel>> {
        use tonic::transport::{Channel, Endpoint};
        use flare_im_core::service_names::{SESSION, get_service_name};
        let mut guard = self.session_service_client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }
        let mut discover_guard = self.session_service_discover.lock().await;
        if discover_guard.is_none() {
            let name = get_service_name(SESSION);
            let discover = flare_im_core::discovery::create_discover(&name).await
                .map_err(|e| CoreFlareError::system(format!("create discover: {}", e)))?;
            if let Some(d) = discover {
                *discover_guard = Some(ServiceClient::new(d));
            }
        }
        let channel: Channel = if let Some(service_client) = discover_guard.as_mut() {
            match service_client.get_channel().await {
                Ok(ch) => ch,
                Err(e) => {
                    let addr = std::env::var("SESSION_GRPC_ADDR").ok().unwrap_or_else(|| "127.0.0.1:50090".to_string());
                    let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                        .map_err(|err| CoreFlareError::system(err.to_string()))?;
                    endpoint.connect().await.map_err(|err| CoreFlareError::system(err.to_string()))?
                }
            }
        } else {
            let addr = std::env::var("SESSION_GRPC_ADDR").ok().unwrap_or_else(|| "127.0.0.1:50090".to_string());
            let endpoint = Endpoint::from_shared(format!("http://{}", addr))
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            endpoint.connect().await.map_err(|err| CoreFlareError::system(err.to_string()))?
        };
        let client = flare_proto::session::session_service_client::SessionServiceClient::new(channel);
        *guard = Some(client.clone());
        Ok(client)
    }
}

impl LongConnectionHandler {
    /// 连接建立时的内部实现
    #[instrument(skip(self), fields(connection_id))]
    async fn on_connect_impl(&self, connection_id: &str) -> CoreResult<()> {
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 更新活跃连接数并获取当前连接数
        let active_count = if let Some(ref handle) = *self.server_handle.lock().await {
            let count = handle.connection_count();
            self.metrics.connections_active.set(count as i64);
            count
        } else {
            0
        };

        // 获取连接信息并记录连接建立日志
        let connection_info = self.get_connection_info(connection_id).await;
        
        if let Some((user_id, device_id)) = connection_info {
            // 连接建立成功：记录关键信息（使用结构化日志）
            info!(
                user_id = %user_id,
                device_id = %device_id,
                connection_id = %connection_id,
                active_connections = active_count,
                "Connection established"
            );
            
            // 注册在线状态到Signaling Online（这会创建会话并存储到Redis，并更新连接信息）
            if let Err(err) = self.register_online_status(&user_id, &device_id, Some(connection_id)).await {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to register online status"
                );
            } else {
                info!(
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Online status registered"
                );
            }
        } else {
            // 连接信息未找到（可能是连接建立过程中出现问题）
            warn!(
                connection_id = %connection_id,
                "Connection established but connection info not found"
            );
        }

        Ok(())
    }

    /// 连接断开时的内部实现
    #[instrument(skip(self), fields(connection_id))]
    async fn on_disconnect_impl(&self, connection_id: &str) -> CoreResult<()> {
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 记录连接断开指标
        self.metrics.connection_disconnected_total.inc();

        // 更新活跃连接数
        let active_count = if let Some(ref handle) = *self.server_handle.lock().await {
            let count = handle.connection_count();
            self.metrics.connections_active.set(count as i64);
            count
        } else {
            0
        };

        // 记录连接断开日志
        info!(
            connection_id = %connection_id,
            active_connections = active_count,
            "Connection disconnected"
        );

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

    /// 处理客户端 ACK 消息
    /// 
    /// 处理客户端发送的 ACK 消息，包括：
    /// - 记录指标
    /// - 上报 ACK 到 Kafka
    /// - 刷新会话心跳
    #[instrument(skip(self), fields(connection_id, message_id = %msg_cmd.message_id))]
    async fn handle_client_ack(
        &self,
        msg_cmd: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<()> {
        let user_id = self
            .user_id_for_connection(connection_id)
            .await
            .unwrap_or_else(|| "unknown".to_string());

        let message_id = msg_cmd.message_id.clone();

        info!(
            "✅ 收到客户端ACK: user_id={}, connection_id={}, message_id={}",
            user_id, connection_id, message_id
        );

        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            let span = tracing::Span::current();
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
                timestamp: chrono::Utc::now().timestamp(),
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

        Ok(())
    }

    /// 提取会话 ID
    /// 
    /// 按优先级从以下位置提取会话 ID：
    /// 1. MessageCommand.metadata["session_id"]
    /// 2. Frame.metadata["session_id"]
    /// 3. 从连接信息中获取（回退方案）
    async fn extract_session_id(
        &self,
        frame: &Frame,
        msg_cmd: &MessageCommand,
        connection_id: &str,
    ) -> String {
        // 首先尝试从 MessageCommand.metadata 中提取
        if let Some(sid) = msg_cmd.metadata.get("session_id")
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
            .filter(|s| !s.is_empty())
        {
            info!(
                session_id = %sid,
                metadata_keys = ?msg_cmd.metadata.keys().collect::<Vec<_>>(),
                metadata_count = msg_cmd.metadata.len(),
                "✅ Using session_id from MessageCommand.metadata"
            );
            return sid;
        }

        // 如果 MessageCommand.metadata 中没有，尝试从 Frame.metadata 中提取
        warn!(
            msg_metadata_keys = ?msg_cmd.metadata.keys().collect::<Vec<_>>(),
            msg_metadata_count = msg_cmd.metadata.len(),
            connection_id = %connection_id,
            "MessageCommand.metadata 中没有有效的 session_id，尝试从 Frame.metadata 获取"
        );

        if let Some(sid) = frame.metadata.get("session_id")
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
            .filter(|s| !s.is_empty())
        {
                        debug!(
                            session_id = %sid,
                            "Using session_id from Frame.metadata"
                        );
            return sid;
        }

        // 如果 Frame.metadata 中也没有，尝试从连接信息中获取（回退方案）
        let fallback_session_id = self.get_session_id_for_connection(connection_id)
            .await
            .unwrap_or_else(|| format!("chatroom:{}", self.gateway_id));

        warn!(
            session_id = %fallback_session_id,
            "使用回退 session_id（可能不是客户端指定的聊天室ID，建议检查客户端是否设置了 metadata.session_id='chatroom'）"
        );

        fallback_session_id
    }

    /// 处理消息发送
    /// 
    /// 处理客户端发送的普通消息，包括：
    /// - 提取会话 ID
    /// - 执行 Hook 检查
    /// - 路由消息到 Message Orchestrator
    #[instrument(skip(self), fields(connection_id))]
    async fn handle_message_send(
        &self,
        frame: &Frame,
        msg_cmd: &MessageCommand,
        connection_id: &str,
    ) -> CoreResult<()> {
        let user_id = self
            .user_id_for_connection(connection_id)
            .await
            .unwrap_or_else(|| "unknown".to_string());

        info!(
            user_id = %user_id,
            connection_id = %connection_id,
            message_len = msg_cmd.payload.len(),
            "Message received"
        );

        // 验证消息大小（在 Gateway 层进行早期验证，避免大消息进入后续处理流程）
        let max_message_size = self.config.max_message_size_bytes;
        if msg_cmd.payload.len() > max_message_size {
            let error_msg = format!(
                "Message size {} bytes exceeds maximum allowed size {} bytes. Please reduce message content size or split into multiple messages.",
                msg_cmd.payload.len(),
                max_message_size
            );
            tracing::warn!(
                user_id = %user_id,
                connection_id = %connection_id,
                message_size = msg_cmd.payload.len(),
                max_size = max_message_size,
                "Message rejected due to size limit"
            );
            return Err(flare_core::common::error::FlareError::message_format_error(error_msg));
        }

        // 路由消息到 Message Orchestrator
        let router = match &self.message_router {
            Some(router) => router,
            None => {
                warn!("Message Router not configured, message will not be routed");
                return Ok(());
            }
        };

        // 提取会话 ID
        let session_id = self.extract_session_id(frame, msg_cmd, connection_id).await;

        // 如果是定向推送（SDK 在 metadata 中附带 target_user_id），优先定向路由
        let mut target_user_id: Option<String> = frame.metadata
            .get("target_user_id")
            .and_then(|tid_bytes| String::from_utf8(tid_bytes.clone()).ok());

        // 调用 Hook（如果注册），允许业务决定投递目标
        target_user_id = self.execute_pre_send_hook(
            &user_id,
            &session_id,
            &msg_cmd.payload,
            target_user_id,
            connection_id,
        ).await?;

        info!(
            user_id = %user_id,
            session_id = %session_id,
            target = ?target_user_id,
            "📨 路由消息到 Message Orchestrator/Direct"
        );

        // 获取租户ID（从连接信息中提取，或使用默认）
        let tenant_id = self.get_tenant_id_for_connection(connection_id).await;

        // 路由消息
        let original_message_id = msg_cmd.message_id.clone();
        let route_res = if let Some(ref target) = target_user_id {
            // 直推模式：将目标用户ID写入扩展字段，由 Orchestrator 进行精准投递
            let payload = self.prepare_direct_message_payload(&msg_cmd.payload, target)?;
            router.route_message(&user_id, &session_id, payload, tenant_id.as_deref()).await
        } else {
            router.route_message(&user_id, &session_id, msg_cmd.payload.clone(), tenant_id.as_deref()).await
        };

        match route_res {
            Ok(response) => {
                info!(
                    user_id = %user_id,
                    session_id = %session_id,
                    message_id = %response.message_id,
                    "Message routed successfully"
                );
                // 发送 ACK 到客户端，标记消息已送达
                self.send_message_ack(connection_id, &response.message_id, &session_id).await?;
            }
            Err(err) => {
                let error_msg = format!("消息发送失败: {}", err);
                tracing::error!(
                    ?err,
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to route message to Message Orchestrator"
                );
                
                // 向客户端发送错误通知
                self.send_error_notification(
                    connection_id,
                    &original_message_id,
                    &error_msg,
                ).await?;
            }
        }

        Ok(())
    }

    /// 执行 pre-send Hook
    /// 
    /// 如果注册了 Hook，执行 pre-send 检查，允许业务决定投递目标
    async fn execute_pre_send_hook(
        &self,
        user_id: &str,
        session_id: &str,
        payload: &[u8],
        mut target_user_id: Option<String>,
        connection_id: &str,
    ) -> CoreResult<Option<String>> {
        use flare_im_core::hooks::{GlobalHookRegistry, HookDispatcher, HookContext, MessageDraft};
        use prost::Message as _;

        let registry = GlobalHookRegistry::get();
        let dispatcher = HookDispatcher::new(registry);
        let mut ctx = HookContext::new(self.gateway_id.clone());
        ctx.sender_id = Some(user_id.to_string());
        ctx.session_id = Some(session_id.to_string());

        let mut draft = MessageDraft::new({
            // 尝试将 payload 解码为 MessageContent bytes；失败则传原始 payload
            if let Ok(m) = flare_proto::Message::decode(payload) {
                let mut buf = Vec::new();
                if let Some(c) = m.content.as_ref() {
                    c.encode(&mut buf).ok();
                }
                buf
            } else {
                payload.to_vec()
            }
        });

        // 将已有定向信息放入草稿，方便 Hook 使用
        if let Some(t) = target_user_id.clone() {
            draft.metadata.insert("receiver_id".into(), t);
        }

        // 执行 pre-send；若拒绝则通知客户端错误并跳过路由
        match dispatcher.registry().execute_pre_send(&ctx, &mut draft).await {
            Ok(()) => {
                if let Some(rid) = draft.metadata.get("receiver_id") {
                    target_user_id = Some(rid.clone());
                } else if let Some(rids) = draft.metadata.get("receiver_ids") {
                    // 取第一个作为定向目标（简化处理）；完整多目标由后续批量推送支持
                    target_user_id = rids.split(',').find(|s| !s.is_empty()).map(|s| s.to_string());
                }
                Ok(target_user_id)
            }
            Err(err) => {
                let error_msg = format!("Hook rejected: {}", err);
                self.send_error_notification(connection_id, "", &error_msg).await?;
                Err(CoreFlareError::system(error_msg))
            }
        }
    }

    /// 准备直推消息的 payload
    /// 
    /// 将目标用户ID写入扩展字段，由 Orchestrator 进行精准投递
    fn prepare_direct_message_payload(
        &self,
        payload: &[u8],
        target_user_id: &str,
    ) -> CoreResult<Vec<u8>> {
        use flare_proto::Message as ProtoMessage;
        use prost::Message as _;

        let mut payload = payload.to_vec();
        if let Ok(mut m) = ProtoMessage::decode(&payload[..]) {
            m.receiver_id = target_user_id.to_string();
            m.receiver_ids = vec![target_user_id.to_string()];
            let mut extra = m.extra;
            extra.insert("direct".to_string(), "1".to_string());
            m.extra = extra;
            payload = m.encode_to_vec();
        }
        Ok(payload)
    }

    /// 发送消息 ACK 到客户端
    async fn send_message_ack(
        &self,
        connection_id: &str,
        message_id: &str,
        session_id: &str,
    ) -> CoreResult<()> {
        use flare_core::common::protocol::{builder::FrameBuilder, Reliability, MessageCommand};
        use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
        
        let mut md = std::collections::HashMap::new();
        md.insert("session_id".to_string(), session_id.as_bytes().to_vec());
        md.insert("delivered".to_string(), b"1".to_vec());
        
        let ack_cmd = MessageCommand {
            r#type: flare_core::common::protocol::flare::core::commands::message_command::Type::Ack as i32,
            message_id: message_id.to_string(),
            payload: vec![],
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
        
        // 获取 handle 并发送（在创建 frame 之后，确保 frame 生命周期正确）
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                warn!("ServerHandle not initialized, cannot send message ACK");
                return Ok(());
            }
        };
        
        handle.send_to(connection_id, &ack_frame).await
            .map_err(|e| CoreFlareError::system(format!("Failed to send message ACK: {}", e)))?;
        
        Ok(())
    }

    /// 发送错误通知到客户端
    async fn send_error_notification(
        &self,
        connection_id: &str,
        original_message_id: &str,
        error_msg: &str,
    ) -> CoreResult<()> {
        use flare_core::common::protocol::{
            frame_with_notification_command, notification,
            flare::core::commands::notification_command::Type as NotificationType,
            Reliability,
        };

        // 先创建 error_frame，确保它在整个函数生命周期内有效
        let mut metadata = std::collections::HashMap::new();
        if !original_message_id.is_empty() {
            metadata.insert("original_message_id".to_string(), original_message_id.as_bytes().to_vec());
        }
        metadata.insert("error_code".to_string(), "ROUTING_FAILED".as_bytes().to_vec());

        let error_notification = notification(
            NotificationType::Alert,
            "消息发送失败".to_string(),
            error_msg.as_bytes().to_vec(),
            Some(metadata),
        );

        let error_frame = frame_with_notification_command(
            error_notification,
            Reliability::AtLeastOnce,
        );

        // 然后获取 handle 并发送（在创建 frame 之后，确保 frame 生命周期正确）
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                warn!("ServerHandle not initialized, cannot send error notification");
                return Ok(());
            }
        };

        if let Err(send_err) = handle.send_to(connection_id, &error_frame).await {
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
}
