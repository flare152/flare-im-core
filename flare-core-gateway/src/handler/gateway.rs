use std::sync::Arc;

use anyhow::Context as AnyhowContext;
use flare_proto::communication_core::{
    GetOnlineStatusRequest, GetOnlineStatusResponse, LoginRequest, LoginResponse,
    PushMessageRequest, PushMessageResponse, PushNotificationRequest, PushNotificationResponse,
    QueryMessagesRequest, QueryMessagesResponse, RouteMessageRequest, RouteMessageResponse,
    StoreMessageRequest, StoreMessageResponse,
};
use flare_proto::push::{
    PushMessageRequest as PushProtoMessageRequest,
    PushNotificationRequest as PushProtoNotificationRequest,
};
use flare_proto::storage::{
    QueryMessagesRequest as StorageQueryMessagesRequest,
    StoreMessageRequest as StorageStoreMessageRequest,
};
use flare_proto::signaling::{
    GetOnlineStatusRequest as SignalingGetOnlineStatusRequest,
    LoginRequest as SignalingLoginRequest,
    RouteMessageRequest as SignalingRouteMessageRequest,
    RouteMessageResponse as SignalingRouteMessageResponse,
    OnlineStatus as SignalingOnlineStatus,
};
use flare_proto::TenantContext;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};

use crate::infrastructure::push::PushClient;
use crate::infrastructure::signaling::SignalingClient;
use crate::infrastructure::storage::StorageClient;
use crate::transform::{
    core_message_to_push_bytes, core_notification_to_push, core_push_options_to_push,
    core_to_storage_message, storage_to_core_message,
};

pub struct GatewayHandler {
    signaling: Arc<dyn SignalingClient>,
    storage: Arc<dyn StorageClient>,
    push: Arc<dyn PushClient>,
}

pub(crate) fn tenant_id_label(tenant: Option<&TenantContext>) -> &str {
    tenant
        .and_then(|ctx| {
            let trimmed = ctx.tenant_id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or("tenant:undefined")
}

fn ensure_tenant(tenant: &Option<TenantContext>, operation: &str) -> Result<()> {
    match tenant {
        Some(ctx) if !ctx.tenant_id.trim().is_empty() => Ok(()),
        _ => Err(ErrorBuilder::new(
            ErrorCode::InvalidParameter,
            "tenant context is required",
        )
        .details(format!("operation={operation}"))
        .build_error()),
    }
}

fn to_signaling_login(request: LoginRequest) -> SignalingLoginRequest {
    SignalingLoginRequest {
        user_id: request.user_id,
        token: request.token,
        device_id: request.device_id,
        server_id: request
            .metadata
            .get("server_id")
            .cloned()
            .unwrap_or_default(),
        metadata: request.metadata,
        context: request.context,
        tenant: request.tenant,
    }
}

fn to_core_online_status(status: SignalingOnlineStatus) -> flare_proto::communication_core::OnlineStatus {
    flare_proto::communication_core::OnlineStatus {
        online: status.online,
        server_id: status.server_id,
        last_seen: status.last_seen,
        tenant: status.tenant,
    }
}

fn convert_push_failure(failure: flare_proto::push::PushFailure) -> flare_proto::communication_core::PushFailure {
    flare_proto::communication_core::PushFailure {
        user_id: failure.user_id,
        code: failure.code,
        error_message: failure.error_message,
    }
}

impl GatewayHandler {
    pub fn new(
        signaling: Arc<dyn SignalingClient>,
        storage: Arc<dyn StorageClient>,
        push: Arc<dyn PushClient>,
    ) -> Self {
        Self {
            signaling,
            storage,
            push,
        }
    }

    pub async fn handle_login(&self, request: LoginRequest) -> Result<LoginResponse> {
        ensure_tenant(&request.tenant, "login")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(tenant = %tenant_label, "gateway handling login request");
        let signaling_request = to_signaling_login(request);
        let response = self.signaling.login(signaling_request).await?;
        Ok(LoginResponse {
            success: response.success,
            session_id: response.session_id,
            error_message: response.error_message,
            status: response.status,
        })
    }

    pub async fn handle_get_online_status(
        &self,
        request: GetOnlineStatusRequest,
    ) -> Result<GetOnlineStatusResponse> {
        ensure_tenant(&request.tenant, "get_online_status")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            "gateway handling get_online_status request"
        );
        let signaling_request = SignalingGetOnlineStatusRequest {
            user_ids: request.user_ids,
            context: request.context,
            tenant: request.tenant,
        };
        let response = self
            .signaling
            .get_online_status(signaling_request)
            .await?;

        Ok(GetOnlineStatusResponse {
            statuses: response
                .statuses
                .into_iter()
                .map(|(user_id, status)| (user_id, to_core_online_status(status)))
                .collect(),
            status: response.status,
        })
    }

    pub async fn handle_route_message(
        &self,
        request: RouteMessageRequest,
    ) -> Result<RouteMessageResponse> {
        ensure_tenant(&request.tenant, "route_message")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            "gateway handling route_message request"
        );
        let signaling_request = SignalingRouteMessageRequest {
            user_id: request.user_id,
            svid: request.svid,
            payload: request.payload,
            context: request.context,
            tenant: request.tenant,
        };

        let response = self
            .signaling
            .route_message(signaling_request)
            .await?;

        Ok(RouteMessageResponse {
            success: response.success,
            response: response.response,
            error_message: response.error_message,
            status: response.status,
        })
    }

    pub async fn handle_store_message(
        &self,
        request: StoreMessageRequest,
    ) -> Result<StoreMessageResponse> {
        let StoreMessageRequest {
            session_id,
            message,
            sync,
            context,
            tenant,
            tags,
        } = request;
        ensure_tenant(&tenant, "store_message")?;
        let tenant_label = tenant_id_label(tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            session_id = %session_id,
            "gateway handling store_message request"
        );

        let core_message = message.context("missing message payload").map_err(|err| {
            ErrorBuilder::new(ErrorCode::InvalidParameter, "store message missing payload")
                .details(err.to_string())
                .build_error()
        })?;

        let storage_request = StorageStoreMessageRequest {
            session_id,
            message: Some(
                core_to_storage_message(&core_message).map_err(|err| {
                    ErrorBuilder::new(ErrorCode::InternalError, "failed to encode message")
                        .details(err.to_string())
                        .build_error()
                })?,
            ),
            sync,
            context,
            tenant,
            tags,
        };

        let response = self.storage.store_message(storage_request).await?;

        Ok(StoreMessageResponse {
            success: response.success,
            message_id: response.message_id,
            error_message: response.error_message,
            status: response.status,
        })
    }

    pub async fn handle_query_messages(
        &self,
        request: QueryMessagesRequest,
    ) -> Result<QueryMessagesResponse> {
        ensure_tenant(&request.tenant, "query_messages")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            session_id = %request.session_id,
            "gateway handling query_messages request"
        );
        let storage_resp = self
            .storage
            .query_messages(StorageQueryMessagesRequest {
                session_id: request.session_id,
                start_time: request.start_time,
                end_time: request.end_time,
                limit: request.limit,
                cursor: request.cursor,
                context: request.context,
                tenant: request.tenant,
                pagination: request.pagination,
            })
            .await?;

        let messages = storage_resp
            .messages
            .into_iter()
            .map(storage_to_core_message)
            .collect();

        Ok(QueryMessagesResponse {
            messages,
            next_cursor: storage_resp.next_cursor,
            has_more: storage_resp.has_more,
            pagination: storage_resp.pagination,
            status: storage_resp.status,
        })
    }

    pub async fn handle_push_message(
        &self,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        ensure_tenant(&request.tenant, "push_message")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            user_count = request.user_ids.len(),
            "gateway handling push_message request"
        );
        let message = request
            .message
            .context("missing message payload")
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::InvalidParameter, "push message payload missing")
                    .details(err.to_string())
                    .build_error()
            })?;
        let payload = core_message_to_push_bytes(&message).map_err(|err| {
            ErrorBuilder::new(ErrorCode::InternalError, "failed to encode push payload")
                .details(err.to_string())
                .build_error()
        })?;

        let push_request = PushProtoMessageRequest {
            user_ids: request.user_ids,
            message: payload,
            options: Some(core_push_options_to_push(&request.options)),
            context: request.context,
            tenant: request.tenant,
        };

        let response = self.push.push_message(push_request).await?;

        Ok(PushMessageResponse {
            success_count: response.success_count,
            fail_count: response.fail_count,
            failed_user_ids: response.failed_user_ids,
            failures: response
                .failures
                .into_iter()
                .map(convert_push_failure)
                .collect(),
            status: response.status,
        })
    }

    pub async fn handle_push_notification(
        &self,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse> {
        ensure_tenant(&request.tenant, "push_notification")?;
        let tenant_label = tenant_id_label(request.tenant.as_ref());
        tracing::debug!(
            tenant = %tenant_label,
            user_count = request.user_ids.len(),
            "gateway handling push_notification request"
        );
        let notification = request
            .notification
            .context("missing notification payload")
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::InvalidParameter, "notification payload missing")
                    .details(err.to_string())
                    .build_error()
            })?;

        let push_request = PushProtoNotificationRequest {
            user_ids: request.user_ids,
            notification: Some(core_notification_to_push(&notification)),
            options: Some(core_push_options_to_push(&request.options)),
            context: request.context,
            tenant: request.tenant,
        };

        let response = self.push.push_notification(push_request).await?;

        Ok(PushNotificationResponse {
            success_count: response.success_count,
            fail_count: response.fail_count,
            failures: response
                .failures
                .into_iter()
                .map(convert_push_failure)
                .collect(),
            status: response.status,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use flare_proto::communication_core::message_content::Content;
    use flare_proto::communication_core::{Message, MessageContent, MessageType, TextContent};
    use flare_proto::push::{PushFailure as PushProtoFailure, PushMessageResponse as PushProtoMessageResponse, PushNotificationResponse as PushProtoNotificationResponse};
    use flare_proto::signaling::{
        GetOnlineStatusRequest, GetOnlineStatusResponse, LoginRequest, LoginResponse,
        LogoutRequest, LogoutResponse, RouteMessageRequest, RouteMessageResponse,
    };
    use flare_proto::storage::{
        BatchStoreMessageRequest, BatchStoreMessageResponse, QueryMessagesRequest as StorageQueryMessagesRequest,
        QueryMessagesResponse as StorageQueryMessagesResponse, StoreMessageRequest as StorageStoreMessageRequest,
        StoreMessageResponse,
    };
    use flare_server_core::error::{ok_status, ErrorBuilder, ErrorCode, Result};
    use std::sync::Mutex;
    use tokio::runtime::Runtime;

    struct StubSignaling;

    #[async_trait]
    impl SignalingClient for StubSignaling {
        async fn login(&self, _: LoginRequest) -> Result<LoginResponse> {
            Ok(LoginResponse {
                success: true,
                session_id: "session-1".into(),
                route_server: "route-1".into(),
                error_message: String::new(),
                status: Some(ok_status()),
            })
        }

        async fn logout(&self, _: LogoutRequest) -> Result<LogoutResponse> {
            Ok(LogoutResponse {
                success: true,
                status: Some(ok_status()),
            })
        }

        async fn get_online_status(
            &self,
            _: GetOnlineStatusRequest,
        ) -> Result<GetOnlineStatusResponse> {
            Ok(GetOnlineStatusResponse {
                statuses: Default::default(),
                status: Some(ok_status()),
            })
        }

        async fn route_message(
            &self,
            _: RouteMessageRequest,
        ) -> Result<RouteMessageResponse> {
            Ok(RouteMessageResponse {
                success: true,
                response: b"OK".to_vec(),
                error_message: String::new(),
                status: Some(ok_status()),
            })
        }
    }

    struct StubStorage {
        last_request: Mutex<Option<StorageStoreMessageRequest>>,
    }

    impl StubStorage {
        fn new() -> Self {
            Self {
                last_request: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl StorageClient for StubStorage {
        async fn store_message(
            &self,
            request: StorageStoreMessageRequest,
        ) -> Result<StoreMessageResponse> {
            let mut guard = self.last_request.lock().unwrap();
            *guard = Some(request);
            Ok(StoreMessageResponse {
                success: true,
                message_id: "msg-1".into(),
                error_message: String::new(),
                status: Some(ok_status()),
            })
        }

        async fn batch_store_message(
            &self,
            _: BatchStoreMessageRequest,
        ) -> Result<BatchStoreMessageResponse> {
            Err(ErrorBuilder::new(
                ErrorCode::OperationNotSupported,
                "not used in tests",
            )
            .build_error())
        }

        async fn query_messages(
            &self,
            _: StorageQueryMessagesRequest,
        ) -> Result<StorageQueryMessagesResponse> {
            Ok(StorageQueryMessagesResponse {
                messages: Vec::new(),
                next_cursor: String::new(),
                has_more: false,
                pagination: None,
                status: Some(ok_status()),
            })
        }
    }

    struct StubPush {
        last_payload: Mutex<Option<Vec<u8>>>,
    }

    impl StubPush {
        fn new() -> Self {
            Self {
                last_payload: Mutex::new(None),
            }
        }
    }

    #[async_trait]
    impl PushClient for StubPush {
        async fn push_message(
            &self,
            request: PushProtoMessageRequest,
        ) -> Result<PushProtoMessageResponse> {
            let mut guard = self.last_payload.lock().unwrap();
            *guard = Some(request.message);
            Ok(PushProtoMessageResponse {
                success_count: 1,
                fail_count: 0,
                failed_user_ids: Vec::new(),
                failures: Vec::new(),
                status: Some(ok_status()),
            })
        }

        async fn push_notification(
            &self,
            _: PushProtoNotificationRequest,
        ) -> Result<PushProtoNotificationResponse> {
            Ok(PushProtoNotificationResponse {
                success_count: 1,
                fail_count: 0,
                failures: Vec::new(),
                status: Some(ok_status()),
            })
        }
    }

    fn test_tenant_ctx() -> Option<TenantContext> {
        Some(TenantContext {
            tenant_id: "tenant-test".into(),
            ..Default::default()
        })
    }

    fn sample_text_message() -> Message {
        Message {
            id: "msg-1".into(),
            session_id: "session-1".into(),
            sender_id: "user-1".into(),
            sender_type: "user".into(),
            receiver_ids: vec!["user-2".into()],
            receiver_id: "user-2".into(),
            content: Some(MessageContent {
                content: Some(Content::Text(TextContent {
                    text: "hello".into(),
                    mentions: Vec::new(),
                })),
            }),
            timestamp: None,
            message_type: MessageType::MessageTypeText as i32,
            business_type: "im".into(),
            session_type: "single".into(),
            status: "created".into(),
            extra: Default::default(),
            is_recalled: false,
            recalled_at: None,
            recall_reason: String::new(),
            is_burn_after_read: false,
            burn_after_seconds: 0,
            read_by: Vec::new(),
            visibility: Default::default(),
            operations: Vec::new(),
            tenant: None,
            attachments: Vec::new(),
            audit: None,
        }
    }

    #[test]
    fn store_message_converts_payload() {
        let rt = Runtime::new().unwrap();
        let handler = GatewayHandler::new(
            Arc::new(StubSignaling),
            Arc::new(StubStorage::new()),
            Arc::new(StubPush::new()),
        );

        let request = StoreMessageRequest {
            session_id: "session-1".into(),
            message: Some(sample_text_message()),
            sync: false,
            context: None,
            tenant: test_tenant_ctx(),
            tags: Default::default(),
        };

        rt.block_on(async {
            let response = handler.handle_store_message(request).await.unwrap();
            assert!(response.success);
        });
    }

    #[test]
    fn push_message_encodes_proto() {
        let rt = Runtime::new().unwrap();
        let push = Arc::new(StubPush::new());
        let handler = GatewayHandler::new(
            Arc::new(StubSignaling),
            Arc::new(StubStorage::new()),
            push.clone(),
        );

        let request = PushMessageRequest {
            user_ids: vec!["user-2".into()],
            message: Some(sample_text_message()),
            options: None,
            context: None,
            tenant: test_tenant_ctx(),
        };

        rt.block_on(async {
            let response = handler.handle_push_message(request).await.unwrap();
            assert_eq!(response.success_count, 1);
        });

        let payload = push.last_payload.lock().unwrap().clone().unwrap();
        let decoded = Message::decode(payload.as_slice()).unwrap();
        assert_eq!(decoded.message_type, MessageType::MessageTypeText as i32);
    }
}

