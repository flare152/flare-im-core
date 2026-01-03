//! Session 服务客户端 - 通过 gRPC 调用 session 服务

use std::sync::Arc;
use anyhow::{Result, Context as AnyhowContext};
use flare_proto::conversation::conversation_service_client::ConversationServiceClient;
use flare_proto::conversation::{CreateConversationRequest, ConversationParticipant, SessionVisibility};
use flare_proto::common::{RequestContext, TenantContext};
use flare_server_core::context::{Context, ContextExt};
use tonic::transport::Channel;
use tracing::{debug, warn, instrument};

use crate::domain::repository::ConversationRepository;

/// Session 服务客户端实现
pub struct GrpcConversationClient {
    client: Arc<tokio::sync::Mutex<ConversationServiceClient<Channel>>>,
}

impl GrpcConversationClient {
    pub fn new(channel: Channel) -> Self {
        Self {
            client: Arc::new(tokio::sync::Mutex::new(ConversationServiceClient::new(channel))),
        }
    }
}


impl ConversationRepository for GrpcConversationClient {
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        conversation_id = %conversation_id,
    ))]
    async fn ensure_conversation(
        &self,
        ctx: &Context,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
    ) -> Result<()> {
        ctx.ensure_not_cancelled().map_err(|e| {
            anyhow::anyhow!("Request cancelled: {}", e)
        })?;
        // 构建 participants
        let session_participants: Vec<ConversationParticipant> = participants
            .into_iter()
            .map(|user_id| ConversationParticipant {
                user_id,
                roles: vec![],
                muted: false,
                pinned: false,
                attributes: std::collections::HashMap::new(),
            })
            .collect();

        // 构建请求
        // 注意：conversation_id 需要通过 attributes 传递，因为 CreateConversationRequest 没有直接的 conversation_id 字段
        // 会话服务会从 attributes 中提取 conversation_id（如果存在）
        let mut attributes = std::collections::HashMap::new();
        attributes.insert("conversation_id".to_string(), conversation_id.to_string());
        
        // 从 Context 中提取 RequestContext 和 TenantContext（用于 protobuf 兼容性）
        let request_context = ctx.request().cloned().unwrap_or_else(|| {
            let request_id = if ctx.request_id().is_empty() {
                uuid::Uuid::new_v4().to_string()
            } else {
                ctx.request_id().to_string()
            };
            RequestContext {
                request_id,
            trace: None,
            actor: None,
            device: None,
            channel: String::new(),
            user_agent: String::new(),
            attributes: std::collections::HashMap::new(),
        });

        let tenant = ctx.tenant().cloned().or_else(|| {
            ctx.tenant_id().map(|tenant_id| TenantContext {
                tenant_id: tenant_id.to_string(),
                business_type: String::new(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            })
        });
        
        let mut request = CreateConversationRequest {
            context: Some(request_context),
            tenant,
            conversation_type: conversation_type.to_string(),
            business_type: business_type.to_string(),
            participants: session_participants,
            attributes,
            visibility: SessionVisibility::SessionVisibilityPrivate as i32,
        };

        let mut client = self.client.lock().await;
        // 调用正确的 RPC 方法：create_conversation（不是 create_session）
        match client.create_conversation(tonic::Request::new(request.clone())).await {
            Ok(response) => {
                let conversation = response.into_inner().conversation;
                if let Some(conv) = conversation {
                    debug!(
                        conversation_id = %conv.conversation_id,
                        "Conversation ensured (created or already exists)"
                    );
                    // 验证返回的 conversation_id 是否与请求的一致
                    if conv.conversation_id != conversation_id {
                        warn!(
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
                    warn!(
                        error = %e,
                        conversation_id = %conversation_id,
                        "Failed to ensure conversation"
                    );
                    Err(anyhow::anyhow!("Failed to ensure conversation: {}", e))
                }
            }
        }
    }
}

