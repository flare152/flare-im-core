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

use crate::domain::repository::SignalingGateway;
use crate::infrastructure::messaging::message_router::MessageRouter;
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::infrastructure::AckPublisher;
use crate::application::services::{ConnectionApplicationService, MessageApplicationService};
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{set_user_id, set_message_id, set_tenant_id};
use tracing::instrument;
use prost::Message as ProstMessage;

/// 长连接处理器
///
/// 处理客户端长连接的消息接收和推送
/// 
/// Gateway 层职责（接口层 - 协议适配）：
/// - 接收和解析协议帧（Frame）
/// - 转换协议对象到领域对象
/// - 委托业务逻辑到应用层服务
/// - 返回响应协议帧
pub struct LongConnectionHandler {
    signaling_gateway: Arc<dyn SignalingGateway>,
    gateway_id: String,
    server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
    manager_trait: Arc<Mutex<Option<Arc<dyn ConnectionManagerTrait>>>>,
    ack_publisher: Option<Arc<dyn AckPublisher>>,
    message_router: Option<Arc<MessageRouter>>,
    ack_sender: Arc<AckSender>,
    metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    session_service_client: Arc<Mutex<Option<flare_proto::session::session_service_client::SessionServiceClient<tonic::transport::Channel>>>>,
    session_service_discover: Arc<Mutex<Option<ServiceClient>>>,
    // 应用层服务
    connection_app_service: Arc<ConnectionApplicationService>,
    message_app_service: Arc<MessageApplicationService>,
}

impl LongConnectionHandler {
    pub fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        gateway_id: String,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        message_router: Option<Arc<MessageRouter>>,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
        connection_app_service: Arc<ConnectionApplicationService>,
        message_app_service: Arc<MessageApplicationService>,
    ) -> Self {
        let server_handle = Arc::new(Mutex::new(None));
        let ack_sender = Arc::new(AckSender::new(server_handle.clone()));
        
        Self {
            signaling_gateway,
            gateway_id,
            server_handle,
            manager_trait: Arc::new(Mutex::new(None)),
            ack_publisher,
            message_router,
            ack_sender,
            metrics,
            session_service_client: Arc::new(Mutex::new(None)),
            session_service_discover: Arc::new(Mutex::new(None)),
            connection_app_service,
            message_app_service,
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
    /// 注意：Gateway 不存储会话信息，会话由 Signaling Online 管理
    /// 这里返回 None，session_id 应该从消息 payload 中提取
    async fn get_session_id_for_connection(&self, _connection_id: &str) -> Option<String> {
        // Gateway 不维护会话信息，会话由 Signaling Online 管理
        // session_id 应该从消息 payload 中提取
        None
    }

    /// 获取连接对应的租户ID
    async fn get_tenant_id_for_connection(&self, _connection_id: &str) -> Option<String> {
        // 从连接信息中提取租户ID（如果连接信息中有）
        // 目前先返回 None，使用默认租户
        None
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
        // Gateway 不维护会话信息，只获取 user_id 用于心跳
        let user_id = match self.user_id_for_connection(connection_id).await {
            Some(user_id) => user_id,
            None => {
                // 连接不存在，可能是连接还未完全建立，不记录错误
                return Ok(());
            }
        };

        // 调用应用层服务刷新心跳，将 flare_server_core::error::Result 转换为 flare_core::common::error::Result
        self.connection_app_service
            .refresh_session(connection_id, &user_id, "")
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to refresh session: {}", e)))
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
    
    async fn on_disconnect(&self, connection_id: &str, _reason: Option<&str>) -> CoreResult<()> {
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
    /// 处理消息帧的内部实现（协议适配层）
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
                        let req = SessionBootstrapRequest::decode(&custom_cmd.data[..])
                            .map_err(|e| CoreFlareError::deserialization_error(format!("decode SessionBootstrapRequest: {}", e)))?;
                        let mut client = self.ensure_session_client().await?;
                        let resp = client.session_bootstrap(req).await
                            .map_err(|status| CoreFlareError::system(status.to_string()))?
                            .into_inner();
                        let mut buf = Vec::new();
                        SessionBootstrapResponse::encode(&resp, &mut buf)
                            .map_err(|e| CoreFlareError::serialization_error(format!("encode SessionBootstrapResponse: {}", e)))?;
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
                        SyncMessagesResponse::encode(&resp, &mut buf)
                            .map_err(|e| CoreFlareError::serialization_error(format!("encode SyncMessagesResponse: {}", e)))?;
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
                        let req = ListSessionsRequest::decode(&custom_cmd.data[..])
                            .map_err(|e| CoreFlareError::deserialization_error(format!("decode ListSessionsRequest: {}", e)))?;
                        let mut client = self.ensure_session_client().await?;
                        let resp = client.list_sessions(req).await
                            .map_err(|status| CoreFlareError::system(status.to_string()))?
                            .into_inner();
                        let mut buf = Vec::new();
                        if let Err(e) = ListSessionsResponse::encode(&resp, &mut buf) {
                            return Err(CoreFlareError::serialization_error(format!("encode ListSessionsResponse: {}", e)));
                        }
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
        }

        Ok(None)
    }

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
                Err(_e) => {
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

    /// 连接建立时的内部实现（协议适配层）
    #[instrument(skip(self), fields(connection_id))]
    async fn on_connect_impl(&self, connection_id: &str) -> CoreResult<()> {
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 获取当前活跃连接数
        let active_count = if let Some(ref handle) = *self.server_handle.lock().await {
            handle.connection_count()
        } else {
            0
        };

        // 获取连接信息
        let connection_info = self.get_connection_info(connection_id).await;
        
        if let Some((user_id, device_id)) = connection_info {
            // 委托给应用层服务处理
            if let Err(err) = self.connection_app_service
                .handle_connect(connection_id, &user_id, &device_id, active_count)
                .await
            {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to handle connection"
                );
            }
        } else {
            warn!(
                connection_id = %connection_id,
                "Connection established but connection info not found"
            );
        }

        Ok(())
    }

    /// 连接断开时的内部实现（协议适配层）
    #[instrument(skip(self), fields(connection_id))]
    async fn on_disconnect_impl(&self, connection_id: &str) -> CoreResult<()> {
        let span = tracing::Span::current();
        span.record("connection_id", connection_id);

        // 获取当前活跃连接数
        let active_count = if let Some(ref handle) = *self.server_handle.lock().await {
            handle.connection_count()
        } else {
            0
        };

        // 获取 user_id
        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            // 检查是否还有其他连接
            let has_other_connections = if let Some(ref manager) = *self.manager_trait.lock().await {
                let count = manager.connection_count().await;
                count > 1
            } else {
                false
            };

            // 委托给应用层服务处理
            if let Err(err) = self.connection_app_service
                .handle_disconnect(connection_id, &user_id, active_count, has_other_connections)
                .await
            {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to handle disconnection"
                );
            }
        }

        Ok(())
    }

    /// 处理客户端 ACK 消息（协议适配层）
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

        // 委托给应用层服务处理
        self.message_app_service
            .handle_client_ack(connection_id, &user_id, msg_cmd)
            .await?;

        // 推送窗口 ACK 更新会话游标（如果提供）
        if let (Some(session_id_bytes), Some(ack_seq_bytes)) = (msg_cmd.metadata.get("session_id"), msg_cmd.metadata.get("ack_seq")) {
            if let (Ok(session_id), Some(ack_seq)) = (String::from_utf8(session_id_bytes.clone()), std::str::from_utf8(ack_seq_bytes.as_slice()).ok().and_then(|s| s.parse::<i64>().ok())) {
                let mut client = self.ensure_session_client().await?;
                let req = flare_proto::session::UpdateCursorRequest {
                    user_id: user_id.clone(),
                    session_id,
                    message_ts: ack_seq,
                    tenant: None,
                    device_id: String::new(),
                };
                let _ = client.update_cursor(tonic::Request::new(req)).await;
            }
        }

        // 刷新会话心跳
        if let Err(err) = self.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session heartbeat");
        }

        Ok(())
    }

    /// 处理消息发送（协议适配层）
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

        // 获取租户ID
        let tenant_id = self.get_tenant_id_for_connection(connection_id).await;

        // 委托给应用层服务处理
        self.message_app_service
            .handle_message_send(connection_id, &user_id, msg_cmd, tenant_id.as_deref())
            .await?;

        Ok(())
    }
}