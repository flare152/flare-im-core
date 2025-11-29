//! Session 服务客户端 - 通过 gRPC 调用 session 服务

use std::sync::Arc;
use anyhow::{Result, Context};
use flare_proto::session::session_service_client::SessionServiceClient;
use flare_proto::session::{CreateSessionRequest, SessionParticipant, SessionVisibility};
use flare_proto::common::RequestContext;
use tonic::transport::Channel;
use tracing::{debug, warn};

use crate::domain::repository::SessionRepository;

/// Session 服务客户端实现
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

        // 构建请求
        let mut request = CreateSessionRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                user_id: String::new(),
                device_id: String::new(),
                client_version: String::new(),
                trace_id: String::new(),
            }),
            tenant: tenant_id.map(|id| flare_proto::common::TenantContext {
                tenant_id: id.to_string(),
            }),
            session_type: session_type.to_string(),
            business_type: business_type.to_string(),
            participants: session_participants,
            attributes: std::collections::HashMap::new(),
            visibility: SessionVisibility::SessionVisibilityPrivate as i32,
        };

        // 注意：session_id 不在 CreateSessionRequest 中，需要从返回的 Session 中获取
        // 但根据业务逻辑，session_id 应该由 session 服务生成或使用传入的 session_id
        // 这里我们假设 session 服务会根据 session_type 和 participants 生成或查找 session_id

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

