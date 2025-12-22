use std::future::Future;
use std::pin::Pin;

use anyhow::Result;
use flare_proto::common::{RequestContext, TenantContext};
use flare_proto::conversation::conversation_service_client::ConversationServiceClient;
use flare_proto::conversation::{CreateConversationRequest, ConversationParticipant};
use tonic::transport::Channel;
use tracing::debug;

use crate::domain::repository::ConversationRepository;

/// gRPC Conversation 客户端（外部依赖）
#[derive(Debug)]
pub struct GrpcConversationClient {
    client: ConversationServiceClient<Channel>,
}

impl GrpcConversationClient {
    pub fn new(client: ConversationServiceClient<Channel>) -> Self {
        Self { client }
    }
}

impl ConversationRepository for GrpcConversationClient {
    fn ensure_conversation(
        &self,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        let conversation_id = conversation_id.to_string();
        let conversation_type = conversation_type.to_string();
        let business_type = business_type.to_string();
        let tenant_id = tenant_id.map(|s| s.to_string());

        // 将 conversation_id 放入 attributes，确保会话服务使用传入的 conversation_id
        let mut attributes = std::collections::HashMap::new();
        attributes.insert("conversation_id".to_string(), conversation_id.clone());

        let request = CreateConversationRequest {
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
            conversation_type: conversation_type.to_string(),
            business_type: business_type.to_string(),
            participants: participants
                .into_iter()
                .map(|p| ConversationParticipant {
                    user_id: p,
                    roles: vec![],
                    muted: false,
                    pinned: false,
                    attributes: std::collections::HashMap::new(),
                })
                .collect(),
            attributes,
            visibility: 0,
        };

        // 注意：ConversationServiceClient 实现了 Clone，可以安全地克隆
        // 但为了更好的错误处理和日志，我们在 async move 之前克隆 client
        // 注意：create_conversation 需要 &mut self，所以我们需要使用 clone() 创建新的客户端实例
        let mut client = self.client.clone();
        Box::pin(async move {
            match client.create_conversation(request).await {
                Ok(response) => {
                    let inner = response.into_inner();
                    if let Some(conv) = inner.conversation {
                        debug!(
                            conversation_id = %conv.conversation_id,
                            "Conversation ensured (created or already exists)"
                        );
                        // 验证返回的 conversation_id 是否与请求的一致
                        if conv.conversation_id != conversation_id {
                            tracing::warn!(
                                requested_conversation_id = %conversation_id,
                                returned_conversation_id = %conv.conversation_id,
                                "Conversation ID mismatch: requested vs returned"
                            );
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    // 如果会话已存在，可能会返回错误，这里我们忽略该错误
                    if e.code() == tonic::Code::AlreadyExists {
                        debug!(conversation_id = %conversation_id, "Conversation already exists, skipping creation");
            Ok(())
                    } else {
                        tracing::warn!(
                            error = %e,
                            conversation_id = %conversation_id,
                            "Failed to ensure conversation"
                        );
                        Err(anyhow::anyhow!("Failed to ensure conversation: {}", e))
                    }
                }
            }
        })
    }
}
