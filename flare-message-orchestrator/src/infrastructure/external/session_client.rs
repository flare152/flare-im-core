//! Session 服务客户端 - 通过 gRPC 调用 session 服务

use std::sync::Arc;
use anyhow::{Result, Context};
use flare_proto::session::session_service_client::SessionServiceClient;
use flare_proto::session::{CreateSessionRequest, SessionParticipant, SessionVisibility};
use flare_proto::common::{RequestContext, ActorContext, ActorType};
use tonic::transport::Channel;
use tracing::{debug, warn};

use crate::domain::repository::SessionRepository;

/// Session 服务客户端实现
#[derive(Debug)]
pub struct GrpcSessionClient {
    client: Arc<tokio::sync::Mutex<SessionServiceClient<Channel>>>,
}

impl GrpcSessionClient {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: Arc::new(tokio::sync::Mutex::new(SessionServiceClient::new(channel))),
        }
    }
}

impl SessionRepository for GrpcSessionClient {
    async fn ensure_session(
        &self,
        session_id: &str,
        session_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Result<()> {
        // 构建 participants
        let session_participants: Vec<SessionParticipant> = participants
            .into_iter()
            .map(|user_id| SessionParticipant {
                user_id,
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: std::collections::HashMap::new(),
            })
            .collect();

        // 构建 attributes，包含 session_id（这样会话服务可以使用指定的 session_id）
        let mut attributes = std::collections::HashMap::new();
        attributes.insert("session_id".to_string(), session_id.to_string());

        // 构建请求
        let request = CreateSessionRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                trace: None,
                actor: Some(ActorContext {
                    actor_id: String::new(),
                    r#type: 2, // ActorType::ActorTypeService
                    roles: vec![],
                    attributes: std::collections::HashMap::new(),
                }),
                device: None,
                channel: String::new(),
                user_agent: String::new(),
                attributes: std::collections::HashMap::new(),
            }),
            tenant: tenant_id.map(|id| flare_proto::common::TenantContext {
                tenant_id: id.to_string(),
                business_type: business_type.to_string(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            session_type: session_type.to_string(),
            business_type: business_type.to_string(),
            participants: session_participants,
            attributes,
            visibility: 1, // SessionVisibility::SessionVisibilityPrivate
        };

        let mut client = self.client.lock().await;
        match client.create_session(tonic::Request::new(request.clone())).await {
            Ok(response) => {
                let session = response.into_inner().session;
                if let Some(s) = session {
                    debug!(
                        session_id = %s.session_id,
                        "Session ensured (created or already exists)"
                    );
                }
                Ok(())
            }
            Err(e) => {
                // 如果 session 已存在，可能会返回错误，这里我们忽略该错误
                if e.code() == tonic::Code::AlreadyExists {
                    debug!(session_id = %session_id, "Session already exists, skipping creation");
                    Ok(())
                } else {
                    warn!(
                        error = %e,
                        session_id = %session_id,
                        "Failed to ensure session"
                    );
                    Err(anyhow::anyhow!("Failed to ensure session: {}", e))
                }
            }
        }
    }
}

