//! 连接管理模块
//!
//! 提供连接处理器的结构定义和连接管理相关方法

use std::sync::Arc;
use flare_core::server::handle::ServerHandle;
use flare_core::server::ConnectionManagerTrait;
use flare_server_core::discovery::ServiceClient;
use tokio::sync::Mutex;
use tracing::warn;

use crate::application::handlers::{ConnectionHandler, MessageHandler};
use crate::domain::repository::SignalingGateway;
use crate::infrastructure::AckPublisher;
use crate::infrastructure::messaging::ack_sender::AckSender;
use crate::infrastructure::messaging::message_router::MessageRouter;

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
    pub(crate) signaling_gateway: Arc<dyn SignalingGateway>,
    pub(crate) gateway_id: String,
    pub(crate) default_tenant_id: String, // 默认租户ID（确保总是存在）
    pub(crate) server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
    pub(crate) manager_trait: Arc<Mutex<Option<Arc<dyn ConnectionManagerTrait>>>>,
    pub(crate) ack_publisher: Option<Arc<dyn AckPublisher>>,
    pub(crate) message_router: Option<Arc<MessageRouter>>,
    pub(crate) ack_sender: Arc<AckSender>,
    pub(crate) metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    pub(crate) conversation_service_client: Arc<
        Mutex<
            Option<
                flare_proto::conversation::conversation_service_client::ConversationServiceClient<
                    tonic::transport::Channel,
                >,
            >,
        >,
    >,
    pub(crate) conversation_service_discover: Arc<Mutex<Option<ServiceClient>>>,
    // 应用层处理器
    pub connection_handler: Arc<ConnectionHandler>,
    pub message_handler: Arc<MessageHandler>,
}

impl LongConnectionHandler {
    pub fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        gateway_id: String,
        default_tenant_id: String,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        message_router: Option<Arc<MessageRouter>>,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
        connection_handler: Arc<ConnectionHandler>,
        message_handler: Arc<MessageHandler>,
    ) -> Self {
        let server_handle = Arc::new(Mutex::new(None));
        let ack_sender = Arc::new(AckSender::new(server_handle.clone()));

        Self {
            signaling_gateway,
            gateway_id,
            default_tenant_id,
            server_handle,
            manager_trait: Arc::new(Mutex::new(None)),
            ack_publisher,
            message_router,
            ack_sender,
            metrics,
            conversation_service_client: Arc::new(Mutex::new(None)),
            conversation_service_discover: Arc::new(Mutex::new(None)),
            connection_handler,
            message_handler,
        }
    }

    /// 带占位符的新构造函数，用于解决循环依赖问题
    pub fn new_with_placeholders(
        signaling_gateway: Arc<dyn SignalingGateway>,
        gateway_id: String,
        default_tenant_id: String,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        message_router: Option<Arc<MessageRouter>>,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    ) -> Self {
        let server_handle = Arc::new(Mutex::new(None));
        let ack_sender = Arc::new(AckSender::new(server_handle.clone()));

        // 创建临时的应用服务实例来打破循环依赖
        let conversation_domain_service = Arc::new(crate::domain::service::conversation_domain_service::ConversationDomainService::new(
            signaling_gateway.clone(),
            Arc::new(crate::domain::service::connection_quality_service::ConnectionQualityService::new()),
            gateway_id.clone(),
        ));

        let connection_handler = Arc::new(ConnectionHandler::new(
            conversation_domain_service.clone(),
            Arc::new(
                crate::infrastructure::connection_query::ManagerConnectionQuery::new(Arc::new(
                    flare_core::server::connection::ConnectionManager::new(),
                )),
            ),
            metrics.clone(),
        ));

        let message_domain_service = Arc::new(crate::domain::service::MessageDomainService::new());
        let message_handler = Arc::new(MessageHandler::new(
            message_domain_service,
            message_router.clone().expect("MessageRouter must be available"),
            ack_publisher.clone(),
            gateway_id.clone(),
        ));

        Self {
            signaling_gateway,
            gateway_id,
            default_tenant_id,
            server_handle,
            manager_trait: Arc::new(Mutex::new(None)),
            ack_publisher,
            message_router,
            ack_sender,
            metrics,
            conversation_service_client: Arc::new(Mutex::new(None)),
            conversation_service_discover: Arc::new(Mutex::new(None)),
            connection_handler,
            message_handler,
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
    pub(crate) async fn get_connection_info(&self, connection_id: &str) -> Option<(String, String)> {
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                // 如果 user_id 为 None，记录警告但不返回 None，而是尝试从其他途径获取
                // 这样可以在连接建立时即使暂时没有 user_id 也能继续处理
                match &conn_info.user_id {
                    Some(user_id) => {
                        let device_id = conn_info
                            .device_info
                            .as_ref()
                            .map(|d| d.device_id.clone())
                            .unwrap_or_else(|| "unknown".to_string());
                        return Some((user_id.clone(), device_id));
                    }
                    None => {
                        // user_id 为 None，这可能是因为认证还在进行中，或者认证失败
                        // 记录警告日志以便调试
                        tracing::warn!(
                            connection_id = %connection_id,
                            authenticated = %conn_info.authenticated,
                            "Connection info found but user_id is None"
                        );
                        return None;
                    }
                }
            }
        }
        None
    }

    /// 获取连接对应的会话ID
    ///
    /// 注意：Gateway 不存储会话信息，会话由 Signaling Online 管理
    /// 这里返回 None，conversation_id 应该从消息 payload 中提取
    pub(crate) async fn get_conversation_id_for_connection(&self, connection_id: &str) -> Option<String> {
        // 从连接管理器中尝试获取会话ID
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                // 尝试从连接信息的元数据中获取会话ID
                let metadata = &conn_info.metadata;
                if let Some(conversation_id) = metadata.get("conversation_id") {
                    return Some(conversation_id.clone());
                }
            }
        }
        // Gateway 不维护会话信息，会话由 Signaling Online 管理
        // conversation_id 应该从消息 payload 中提取
        None
    }

    /// 获取连接对应的租户ID（确保总是返回一个值）
    ///
    /// 从连接信息的 metadata 中提取租户ID，如果没有则返回默认值
    pub(crate) async fn get_tenant_id_for_connection(&self, connection_id: &str) -> String {
        // 从连接信息的 metadata 中提取租户ID
        if let Some(metadata) = self.get_connection_metadata(connection_id).await {
            if let Some(tenant_id) = crate::infrastructure::connection_context::extract_tenant_id_from_metadata(&metadata) {
                return tenant_id;
            }
        }
        // 如果没有找到，返回默认值
        self.default_tenant_id.clone()
    }
    
    /// 获取连接的 metadata（内部辅助函数）
    pub(crate) async fn get_connection_metadata(
        &self,
        connection_id: &str,
    ) -> Option<std::collections::HashMap<String, String>> {
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                return Some(conn_info.metadata.clone());
            }
        }
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
    pub async fn refresh_session(&self, connection_id: &str) -> flare_core::common::error::Result<()> {
        use flare_core::common::error::FlareError as CoreFlareError;

        // Gateway 不维护会话信息，只获取 user_id 和 conversation_id 用于心跳
        let user_id = match self.user_id_for_connection(connection_id).await {
            Some(user_id) => user_id,
            None => {
                // 连接不存在，可能是连接还未完全建立，不记录错误
                return Ok(());
            }
        };

        // 获取会话ID
        let conversation_id = match self.get_conversation_id_for_connection(connection_id).await {
            Some(conversation_id) => conversation_id,
            None => {
                // 没有会话ID，可能是连接还未完全建立，不记录错误
                return Ok(());
            }
        };

        // 调用应用层服务刷新心跳，将 flare_server_core::error::Result 转换为 flare_core::common::error::Result
        self.connection_handler
            .refresh_session(connection_id, &user_id, &conversation_id)
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to refresh session: {}", e)))
    }
}
