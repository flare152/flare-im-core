//! 推送领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::push::{
    PushFailure, PushMessageRequest, PushMessageResponse, PushNotificationRequest,
    PushNotificationResponse,
};
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::domain::repositories::PushEventPublisher;
use crate::infrastructure::validator::RequestValidator;

/// 推送领域服务 - 包含所有业务逻辑
pub struct PushDomainService {
    publisher: Arc<dyn PushEventPublisher>,
    validator: Arc<dyn RequestValidator>,
}

impl PushDomainService {
    pub fn new(
        publisher: Arc<dyn PushEventPublisher>,
        validator: Arc<dyn RequestValidator>,
    ) -> Self {
        Self {
            publisher,
            validator,
        }
    }

    /// 入队推送消息（业务逻辑）
    #[instrument(skip(self), fields(user_count = request.user_ids.len()))]
    pub async fn enqueue_message(
        &self,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        // 1. 入参校验
        self.validator
            .validate_message_request(&request)
            .context("Request validation failed")?;

        let user_ids = request.user_ids.clone();
        let task_id = Uuid::new_v4().to_string();

        // 2. 发布到 Kafka（幂等性由 Kafka 保证）
        match self.publisher.publish_message(&request).await {
            Ok(_) => {
                // 3. PostSend Hook（异步，不阻塞响应）
                // 注意：PostSend Hook 在 proxy 中只做审计日志，不修改消息状态
                // 实际的消息状态由 server/worker 处理
                tokio::spawn({
                    let _request = request.clone();
                    let task_id = task_id.clone();
                    let user_count = user_ids.len();
                    async move {
                        // TODO: 调用 PostSend Hook（审计日志）
                        tracing::info!(
                            task_id = %task_id,
                            user_count = user_count,
                            "Push message enqueued successfully"
                        );
                    }
                });

                Ok(PushMessageResponse {
                    success_count: user_ids.len() as i32,
                    fail_count: 0,
                    failed_user_ids: Vec::new(),
                    failures: Vec::new(),
                    task_id,
                    status: Some(rpc_status_success()),
                })
            }
            Err(err) => {
                warn!(
                    error = %err,
                    task_id = %task_id,
                    "Failed to publish message to Kafka"
                );

                let failures: Vec<PushFailure> = user_ids
                    .iter()
                    .map(|user_id| PushFailure {
                        user_id: user_id.clone(),
                        code: error_code_internal(),
                        error_message: err.to_string(),
                        metadata: std::collections::HashMap::new(),
                    })
                    .collect();

                Ok(PushMessageResponse {
                    success_count: 0,
                    fail_count: user_ids.len() as i32,
                    failed_user_ids: user_ids.clone(),
                    failures,
                    task_id: String::new(),
                    status: Some(rpc_status_internal("failed to enqueue push message")),
                })
            }
        }
    }

    /// 入队推送通知（业务逻辑）
    #[instrument(skip(self), fields(user_count = request.user_ids.len()))]
    pub async fn enqueue_notification(
        &self,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse> {
        // 1. 入参校验
        self.validator
            .validate_notification_request(&request)
            .context("Request validation failed")?;

        let user_ids = request.user_ids.clone();
        let task_id = Uuid::new_v4().to_string();

        // 2. 发布到 Kafka（幂等性由 Kafka 保证）
        match self.publisher.publish_notification(&request).await {
            Ok(_) => {
                // 3. PostSend Hook（异步，不阻塞响应）
                tokio::spawn({
                    let _request = request.clone();
                    let task_id = task_id.clone();
                    let user_count = user_ids.len();
                    async move {
                        // TODO: 调用 PostSend Hook（审计日志）
                        tracing::info!(
                            task_id = %task_id,
                            user_count = user_count,
                            "Push notification enqueued successfully"
                        );
                    }
                });

                Ok(PushNotificationResponse {
                    success_count: user_ids.len() as i32,
                    fail_count: 0,
                    failures: Vec::new(),
                    task_id,
                    status: Some(rpc_status_success()),
                })
            }
            Err(err) => {
                warn!(
                    error = %err,
                    task_id = %task_id,
                    "Failed to publish notification to Kafka"
                );

                let failures: Vec<PushFailure> = user_ids
                    .iter()
                    .map(|user_id| PushFailure {
                        user_id: user_id.clone(),
                        code: error_code_internal(),
                        error_message: err.to_string(),
                        metadata: std::collections::HashMap::new(),
                    })
                    .collect();

                Ok(PushNotificationResponse {
                    success_count: 0,
                    fail_count: user_ids.len() as i32,
                    failures,
                    task_id: String::new(),
                    status: Some(rpc_status_internal("failed to enqueue push notification")),
                })
            }
        }
    }
}

fn error_code_internal() -> i32 {
    flare_proto::common::ErrorCode::Internal as i32
}

fn rpc_status_success() -> flare_proto::common::RpcStatus {
    flare_proto::common::RpcStatus {
        code: flare_proto::common::ErrorCode::Ok as i32,
        message: String::new(),
        details: Default::default(),
        context: None,
    }
}

fn rpc_status_internal(message: &str) -> flare_proto::common::RpcStatus {
    flare_proto::common::RpcStatus {
        code: error_code_internal(),
        message: message.to_string(),
        details: Default::default(),
        context: None,
    }
}
