use std::sync::Arc;

use anyhow::Result;
use flare_proto::push::{
    PushFailure, PushMessageRequest, PushMessageResponse, PushNotificationRequest,
    PushNotificationResponse,
};

use crate::domain::repositories::PushEventPublisher;

pub struct PushCommandService {
    publisher: Arc<dyn PushEventPublisher>,
}

impl PushCommandService {
    pub fn new(publisher: Arc<dyn PushEventPublisher>) -> Self {
        Self { publisher }
    }

    pub async fn enqueue_message(
        &self,
        request: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        let user_ids = request.user_ids.clone();
        if let Err(err) = self.publisher.publish_message(&request).await {
            let failures: Vec<PushFailure> = user_ids
                .iter()
                .map(|user_id| PushFailure {
                    user_id: user_id.clone(),
                    code: error_code_internal(),
                    error_message: err.to_string(),
                })
                .collect();
            return Ok(PushMessageResponse {
                success_count: 0,
                fail_count: user_ids.len() as i32,
                failed_user_ids: user_ids,
                failures,
                status: Some(rpc_status_internal("failed to enqueue push message")),
            });
        }

        Ok(PushMessageResponse {
            success_count: user_ids.len() as i32,
            fail_count: 0,
            failed_user_ids: Vec::new(),
            failures: Vec::new(),
            status: rpc_status_success(),
        })
    }

    pub async fn enqueue_notification(
        &self,
        request: PushNotificationRequest,
    ) -> Result<PushNotificationResponse> {
        let user_ids = request.user_ids.clone();
        if let Err(err) = self.publisher.publish_notification(&request).await {
            let failures: Vec<PushFailure> = user_ids
                .iter()
                .map(|user_id| PushFailure {
                    user_id: user_id.clone(),
                    code: error_code_internal(),
                    error_message: err.to_string(),
                })
                .collect();
            return Ok(PushNotificationResponse {
                success_count: 0,
                fail_count: user_ids.len() as i32,
                failures,
                status: Some(rpc_status_internal("failed to enqueue push notification")),
            });
        }

        Ok(PushNotificationResponse {
            success_count: user_ids.len() as i32,
            fail_count: 0,
            failures: Vec::new(),
            status: rpc_status_success(),
        })
    }
}

fn error_code_internal() -> i32 {
    flare_proto::common::ErrorCode::Internal as i32
}

fn rpc_status_success() -> Option<flare_proto::common::RpcStatus> {
    Some(flare_proto::common::RpcStatus {
        code: flare_proto::common::ErrorCode::Ok as i32,
        message: String::new(),
        details: Default::default(),
    })
}

fn rpc_status_internal(message: &str) -> flare_proto::common::RpcStatus {
    flare_proto::common::RpcStatus {
        code: error_code_internal(),
        message: message.to_string(),
        details: Default::default(),
    }
}
