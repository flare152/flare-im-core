pub mod noop;

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

use crate::config::PushWorkerConfig;
use crate::domain::model::PushDispatchTask;
use crate::domain::repository::OfflinePushSender;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};

pub type OfflinePushSenderRef = Arc<dyn OfflinePushSender>;

pub fn build_offline_sender(config: &PushWorkerConfig) -> OfflinePushSenderRef {
    match config.push_provider.as_str() {
        "fcm" => FcmOfflinePushSender::new(),
        "apns" => ApnsOfflinePushSender::new(),
        "webpush" => WebPushOfflinePushSender::new(),
        _ => noop::NoopOfflinePushSender::shared(),
    }
}

pub use noop::NoopOfflinePushSender;

// FCM推送发送器
pub struct FcmOfflinePushSender {
    client: Client,
}

impl FcmOfflinePushSender {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: Client::new(),
        })
    }
}

#[async_trait]
impl OfflinePushSender for FcmOfflinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        // 获取FCM配置信息（从task.metadata中获取）
        let fcm_token = task.metadata.get("fcm_token").ok_or_else(|| {
            ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "FCM token not found in task metadata",
            )
            .build_error()
        })?;

        // 构建FCM推送消息
        let message = serde_json::json!({
            "message": {
                "token": fcm_token,
                "notification": {
                    "title": "New Message",
                    "body": "You have a new message"
                },
                "data": {
                    "message_id": task.message_id,
                    "user_id": task.user_id,
                    "payload": base64::encode(&task.message)
                }
            }
        });

        // 实际调用FCM API发送推送
        // 这里应该使用HTTP客户端发送POST请求到FCM服务器
        let fcm_api_key = std::env::var("FCM_API_KEY").map_err(|_| {
            ErrorBuilder::new(
                ErrorCode::ConfigurationError,
                "FCM_API_KEY environment variable not set",
            )
            .build_error()
        })?;

        let response = self
            .client
            .post("https://fcm.googleapis.com/v1/projects/myproject/messages:send")
            .header("Authorization", format!("Bearer {}", fcm_api_key))
            .json(&message)
            .send()
            .await
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to send FCM push notification",
                )
                .details(e.to_string())
                .build_error()
            })?;

        if response.status().is_success() {
            tracing::info!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                "FCM offline push sent successfully"
            );
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                error = %error_text,
                "Failed to send FCM offline push"
            );
            return Err(ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "FCM push notification failed",
            )
            .details(error_text)
            .build_error());
        }

        Ok(())
    }
}

// APNs推送发送器
pub struct ApnsOfflinePushSender {
    client: Client,
}

impl ApnsOfflinePushSender {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: Client::new(),
        })
    }
}

#[async_trait]
impl OfflinePushSender for ApnsOfflinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        // 获取APNs配置信息（从task.metadata中获取）
        let apns_token = task.metadata.get("apns_token").ok_or_else(|| {
            ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "APNs token not found in task metadata",
            )
            .build_error()
        })?;

        // 构建APNs推送消息
        let message = serde_json::json!({
            "aps": {
                "alert": {
                    "title": "New Message",
                    "body": "You have a new message"
                },
                "badge": 1,
                "sound": "default"
            },
            "message_id": task.message_id,
            "user_id": task.user_id,
            "payload": base64::encode(&task.message)
        });

        // 实际调用APNs API发送推送
        // 这里应该使用HTTP/2客户端发送POST请求到APNs服务器
        let apns_auth_key = std::env::var("APNS_AUTH_KEY").map_err(|_| {
            ErrorBuilder::new(
                ErrorCode::ConfigurationError,
                "APNS_AUTH_KEY environment variable not set",
            )
            .build_error()
        })?;

        let response = self
            .client
            .post("https://api.push.apple.com/3/device/")
            .header("Authorization", format!("Bearer {}", apns_auth_key))
            .json(&message)
            .send()
            .await
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to send APNs push notification",
                )
                .details(e.to_string())
                .build_error()
            })?;

        if response.status().is_success() {
            tracing::info!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                "APNs offline push sent successfully"
            );
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                error = %error_text,
                "Failed to send APNs offline push"
            );
            return Err(ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "APNs push notification failed",
            )
            .details(error_text)
            .build_error());
        }

        Ok(())
    }
}

// WebPush推送发送器
pub struct WebPushOfflinePushSender {
    client: Client,
}

impl WebPushOfflinePushSender {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            client: Client::new(),
        })
    }
}

#[async_trait]
impl OfflinePushSender for WebPushOfflinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        // 获取WebPush配置信息（从task.metadata中获取）
        let subscription = task.metadata.get("webpush_subscription").ok_or_else(|| {
            ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "WebPush subscription not found in task metadata",
            )
            .build_error()
        })?;

        // 解析订阅信息
        let subscription_value: Value = serde_json::from_str(subscription).map_err(|e| {
            ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "Invalid WebPush subscription JSON",
            )
            .details(e.to_string())
            .build_error()
        })?;

        let endpoint = subscription_value
            .get("endpoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                ErrorBuilder::new(
                    ErrorCode::InvalidParameter,
                    "WebPush subscription missing endpoint",
                )
                .build_error()
            })?;

        // 构建WebPush推送消息
        let message = serde_json::json!({
            "notification": {
                "title": "New Message",
                "body": "You have a new message"
            },
            "data": {
                "message_id": task.message_id,
                "user_id": task.user_id,
                "payload": base64::encode(&task.message)
            }
        });

        // 实际调用WebPush API发送推送
        // 这里应该使用HTTP客户端发送POST请求到WebPush服务器
        let response = self
            .client
            .post(endpoint)
            .json(&message)
            .send()
            .await
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "Failed to send WebPush notification",
                )
                .details(e.to_string())
                .build_error()
            })?;

        if response.status().is_success() {
            tracing::info!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                "WebPush offline push sent successfully"
            );
        } else {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            tracing::error!(
                user_id = %task.user_id,
                message_id = %task.message_id,
                error = %error_text,
                "Failed to send WebPush offline push"
            );
            return Err(ErrorBuilder::new(
                ErrorCode::ServiceUnavailable,
                "WebPush notification failed",
            )
            .details(error_text)
            .build_error());
        }

        Ok(())
    }
}
