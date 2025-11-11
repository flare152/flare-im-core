use std::collections::HashMap;
use std::sync::Arc;

use flare_im_core::hooks::HookDispatcher;
use flare_proto::push::{PushMessageRequest, PushNotificationRequest, PushOptions};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tracing::{info, warn};

use crate::domain::models::{DispatchNotification, PushDispatchTask, RequestMetadata};
use crate::domain::repositories::{OnlineStatusRepository, PushTaskPublisher};
use crate::hooks::{
    build_post_send_record, finalize_notification, prepare_message_envelope,
    prepare_notification_envelope,
};
use crate::infrastructure::config::PushServerConfig;

pub struct PushDispatchCommandService {
    config: Arc<PushServerConfig>,
    online_repo: Arc<dyn OnlineStatusRepository>,
    task_publisher: Arc<dyn PushTaskPublisher>,
    hooks: Arc<HookDispatcher>,
}

impl PushDispatchCommandService {
    pub fn new(
        config: Arc<PushServerConfig>,
        online_repo: Arc<dyn OnlineStatusRepository>,
        task_publisher: Arc<dyn PushTaskPublisher>,
        hooks: Arc<HookDispatcher>,
    ) -> Self {
        Self {
            config,
            online_repo,
            task_publisher,
            hooks,
        }
    }

    pub async fn handle_push_message(&self, request: PushMessageRequest) -> Result<()> {
        let tenant_ctx = request.tenant.clone();
        let tenant_ref = request.tenant.as_ref();
        let context_ref = request.context.as_ref();
        let metadata = context_ref.map(request_metadata_from_context);

        for user_id in &request.user_ids {
            let online = match self.online_repo.is_online(user_id).await {
                Ok(online) => online,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to load online state");
                    false
                }
            };

            if !online && !should_persist_offline(&request.options) {
                info!(user_id = %user_id, "skip offline user due to persist_if_offline = false");
                continue;
            }

            let (envelope, mut draft) = prepare_message_envelope(
                &self.config.default_tenant_id,
                tenant_ref,
                user_id,
                context_ref,
                request.options.as_ref(),
                request.message.clone(),
            );

            if let Err(err) = self
                .hooks
                .pre_send(envelope.hook_context(), &mut draft)
                .await
            {
                warn!(user_id = %user_id, ?err, "pre-dispatch hook rejected push message");
                continue;
            }

            let message_id = envelope.message_id(&draft);
            let message_type_label = draft
                .metadata
                .get("message_type")
                .cloned()
                .unwrap_or_else(|| envelope.message_type().to_string());
            let task = self.build_task(
                user_id.clone(),
                message_id.clone(),
                message_type_label,
                draft.payload.clone(),
                None,
                draft.headers.clone(),
                draft.metadata.clone(),
                online,
                request.options.as_ref(),
                tenant_identifier(tenant_ctx.as_ref(), &self.config.default_tenant_id),
                metadata.clone(),
            );

            self.task_publisher.publish(&task).await?;

            if let Err(err) = self
                .hooks
                .post_send(
                    envelope.hook_context(),
                    &build_post_send_record(&envelope, &draft),
                    &draft,
                )
                .await
            {
                warn!(user_id = %user_id, ?err, "post-dispatch hook failed for push message");
            }
        }

        Ok(())
    }

    pub async fn handle_push_notification(&self, request: PushNotificationRequest) -> Result<()> {
        let tenant_ctx = request.tenant.clone();
        let tenant_ref = request.tenant.as_ref();
        let context_ref = request.context.as_ref();
        let metadata = context_ref.map(request_metadata_from_context);

        let base_notification = match request.notification.clone() {
            Some(notification) => DispatchNotification::from(notification),
            None => {
                return Err(ErrorBuilder::new(
                    ErrorCode::InvalidParameter,
                    "notification payload is required",
                )
                .build_error());
            }
        };

        for user_id in &request.user_ids {
            let online = match self.online_repo.is_online(user_id).await {
                Ok(online) => online,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to load online state");
                    false
                }
            };

            if !online && !should_persist_offline(&request.options) {
                info!(user_id = %user_id, "skip offline notification due to persist_if_offline = false");
                continue;
            }

            let preparation = prepare_notification_envelope(
                &self.config.default_tenant_id,
                tenant_ref,
                user_id,
                context_ref,
                request.options.as_ref(),
                &base_notification,
            );

            let (envelope, mut draft) = match preparation {
                Ok(result) => result,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to prepare notification hook payload");
                    continue;
                }
            };

            if let Err(err) = self
                .hooks
                .pre_send(envelope.hook_context(), &mut draft)
                .await
            {
                warn!(user_id = %user_id, ?err, "pre-dispatch hook rejected notification");
                continue;
            }

            let final_notification = match finalize_notification(&draft) {
                Ok(notification) => notification,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to decode notification after hook");
                    continue;
                }
            };

            let message_id = envelope.message_id(&draft);
            let task = self.build_task(
                user_id.clone(),
                message_id.clone(),
                envelope.message_type().to_string(),
                Vec::new(),
                Some(final_notification),
                draft.headers.clone(),
                draft.metadata.clone(),
                online,
                request.options.as_ref(),
                tenant_identifier(tenant_ctx.as_ref(), &self.config.default_tenant_id),
                metadata.clone(),
            );

            self.task_publisher.publish(&task).await?;

            if let Err(err) = self
                .hooks
                .post_send(
                    envelope.hook_context(),
                    &build_post_send_record(&envelope, &draft),
                    &draft,
                )
                .await
            {
                warn!(user_id = %user_id, ?err, "post-dispatch hook failed for notification");
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_task(
        &self,
        user_id: String,
        message_id: String,
        message_type: String,
        message: Vec<u8>,
        notification: Option<DispatchNotification>,
        headers: HashMap<String, String>,
        metadata: HashMap<String, String>,
        online: bool,
        options: Option<&PushOptions>,
        tenant_id: Option<String>,
        context: Option<RequestMetadata>,
    ) -> PushDispatchTask {
        let (require_online, persist_if_offline, priority) = options
            .map(|opt| (opt.require_online, opt.persist_if_offline, opt.priority))
            .unwrap_or((false, true, 0));

        PushDispatchTask {
            user_id,
            message_id,
            message_type,
            message,
            notification,
            headers,
            metadata,
            online,
            tenant_id,
            require_online,
            persist_if_offline,
            priority,
            context,
        }
    }
}

impl From<flare_proto::push::Notification> for DispatchNotification {
    fn from(value: flare_proto::push::Notification) -> Self {
        Self {
            title: value.title,
            body: value.body,
            data: value.data,
            metadata: value.metadata,
        }
    }
}

fn tenant_identifier(
    tenant: Option<&flare_proto::common::TenantContext>,
    default: &str,
) -> Option<String> {
    tenant
        .and_then(|tenant| {
            if tenant.tenant_id.is_empty() {
                None
            } else {
                Some(tenant.tenant_id.clone())
            }
        })
        .or_else(|| Some(default.to_string()))
}

fn request_metadata_from_context(context: &flare_proto::common::RequestContext) -> RequestMetadata {
    fn to_opt(value: &str) -> Option<String> {
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    RequestMetadata {
        request_id: context.request_id.clone(),
        trace_id: to_opt(&context.trace_id),
        span_id: to_opt(&context.span_id),
        client_ip: to_opt(&context.client_ip),
        user_agent: to_opt(&context.user_agent),
    }
}

fn should_persist_offline(options: &Option<PushOptions>) -> bool {
    options
        .as_ref()
        .map(|opt| opt.persist_if_offline)
        .unwrap_or(true)
}
