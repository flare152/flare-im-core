//! Session 服务客户端（用于查询会话参与者）

use std::sync::Arc;

use flare_proto::session::session_service_client::SessionServiceClient as SessionServiceClientProto;
use flare_proto::session::{UpdateSessionRequest, UpdateSessionResponse};
use flare_proto::common::{RequestContext, ActorContext};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use flare_server_core::discovery::ServiceClient;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::debug;

/// Session 服务客户端
pub struct SessionServiceClient {
    service_name: String,
    service_client: Mutex<Option<ServiceClient>>,
    client: Mutex<Option<SessionServiceClientProto<Channel>>>,
}

impl SessionServiceClient {
    /// 创建新的客户端（使用服务名称，内部创建服务发现）
    pub fn new(service_name: String) -> Arc<Self> {
        Arc::new(Self {
            service_name,
            service_client: Mutex::new(None),
            client: Mutex::new(None),
        })
    }

    /// 使用 ServiceClient 创建新的客户端（推荐，通过 wire 注入）
    pub fn with_service_client(service_client: ServiceClient) -> Arc<Self> {
        Arc::new(Self {
            service_name: String::new(), // 不需要 service_name
            service_client: Mutex::new(Some(service_client)),
            client: Mutex::new(None),
        })
    }

    async fn ensure_client(&self) -> Result<SessionServiceClientProto<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        // 使用服务发现获取 Channel
        let mut service_client_guard = self.service_client.lock().await;
        if service_client_guard.is_none() {
            // 如果没有注入 ServiceClient，则创建服务发现器
            let discover = flare_im_core::discovery::create_discover(&self.service_name)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session service unavailable")
                        .details(format!("Failed to create service discover for {}: {}", self.service_name, e))
                        .build_error()
                })?;
            
            if let Some(discover) = discover {
                *service_client_guard = Some(ServiceClient::new(discover));
            } else {
                return Err(ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session service unavailable")
                    .details("Service discovery not configured")
                    .build_error());
            }
        }
        
        let service_client = service_client_guard.as_mut().ok_or_else(|| {
            ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session service unavailable")
                .details("Service client not initialized")
                .build_error()
        })?;
        // 添加超时保护，避免服务发现阻塞过长时间
        let channel = tokio::time::timeout(
            std::time::Duration::from_secs(3), // 3秒超时
            service_client.get_channel()
        ).await
            .map_err(|_| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session service unavailable")
                    .details("Timeout waiting for service discovery to get channel (3s)")
                    .build_error()
            })?
            .map_err(|e| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session service unavailable")
                    .details(format!("Failed to get channel: {}", e))
                    .build_error()
            })?;
        
        debug!("Got channel for session service from service discovery");

        let client = SessionServiceClientProto::new(channel);
        *guard = Some(client.clone());
        Ok(client)
    }

    /// 获取会话的所有参与者（通过 UpdateSession 方法，只传 session_id 获取 Session 信息）
    pub async fn get_session_participants(
        &self,
        session_id: &str,
        tenant_id: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut client = self.ensure_client().await?;

        // 使用 UpdateSession 方法，只传 session_id，其他字段留空，来获取 Session 信息
        let request = UpdateSessionRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                trace: None,
                actor: Some(ActorContext {
                    actor_id: String::new(),
                    r#type: 2, // ActorType::ACTOR_TYPE_SERVICE
                    roles: vec![],
                    attributes: std::collections::HashMap::new(),
                }),
                device: None,
                channel: String::new(),
                user_agent: String::new(),
                attributes: std::collections::HashMap::new(),
            }),
            tenant: tenant_id.map(|tid| flare_proto::common::TenantContext {
                tenant_id: tid.to_string(),
                business_type: String::new(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            session_id: session_id.to_string(),
            display_name: String::new(), // 留空，不更新
            attributes: std::collections::HashMap::new(), // 留空，不更新
            visibility: 0, // 留空，不更新
            lifecycle_state: 0, // 留空，不更新
        };

        let response: UpdateSessionResponse = client
            .update_session(tonic::Request::new(request))
            .await
            .map_err(|status| {
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "session query failed")
                    .details(format!("Failed to get session participants: {}", status))
                    .build_error()
            })?
            .into_inner();

        // 从 Session 中提取参与者 user_id 列表
        if let Some(session) = response.session {
            Ok(session
                .participants
                .into_iter()
                .map(|p| p.user_id)
                .collect())
        } else {
            Err(ErrorBuilder::new(ErrorCode::InvalidParameter, "session not found")
                .details(format!("Session {} not found", session_id))
                .build_error())
        }
    }
}

