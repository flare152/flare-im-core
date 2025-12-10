use std::pin::Pin;
use std::future::Future;

use anyhow::Result;
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::session::session_service_client::SessionServiceClient;
use flare_proto::session::{CreateSessionRequest, SessionParticipant};
use tonic::transport::Channel;
use tracing::debug;

use crate::domain::repository::SessionRepository;

/// gRPC Session 客户端（外部依赖）
#[derive(Debug)]
pub struct GrpcSessionClient {
    client: SessionServiceClient<Channel>,
}

impl GrpcSessionClient {
    pub fn new(client: SessionServiceClient<Channel>) -> Self {
        Self { client }
    }
}

impl SessionRepository for GrpcSessionClient {
    fn ensure_session(
        &self,
        session_id: &str,
        session_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        let session_id = session_id.to_string();
        let session_type = session_type.to_string();
        let business_type = business_type.to_string();
        let tenant_id = tenant_id.map(|s| s.to_string());
        
        let request = CreateSessionRequest {
            context: Some(RequestContext {
                request_id: uuid::Uuid::new_v4().to_string(),
                trace: None,
                actor: None,
                device: None,
                channel: String::new(),
                user_agent: String::new(),
                attributes: std::collections::HashMap::new(),
            }),
            tenant: tenant_id.as_deref().map(|id| TenantContext {
                tenant_id: id.to_string(),
                business_type: String::new(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            session_type: session_type.to_string(),
            business_type: business_type.to_string(),
            participants: participants.into_iter().map(|p| SessionParticipant {
                user_id: p,
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: std::collections::HashMap::new(),
            }).collect(),
            attributes: std::collections::HashMap::new(),
            visibility: 0,
        };

        Box::pin(async move {
            let response = self.client.clone().create_session(request).await?;
            let _inner = response.into_inner();

            debug!(session_id = %session_id, "Ensured session exists");
            Ok(())
        })
    }
}