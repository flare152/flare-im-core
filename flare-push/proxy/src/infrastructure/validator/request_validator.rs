//! 请求校验器实现

use anyhow::{ensure, Result};
use flare_proto::push::{PushMessageRequest, PushNotificationRequest};

/// 请求校验器实现
pub struct RequestValidatorImpl;

impl RequestValidatorImpl {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RequestValidatorImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl crate::infrastructure::validator::RequestValidator for RequestValidatorImpl {
    fn validate_message_request(&self, request: &PushMessageRequest) -> Result<()> {
        // 校验用户ID列表不为空
        ensure!(
            !request.user_ids.is_empty(),
            "user_ids cannot be empty"
        );

        // 校验用户ID数量限制（防止批量过大）
        ensure!(
            request.user_ids.len() <= 1000,
            "user_ids count exceeds maximum limit of 1000"
        );

        // 校验消息内容不为空（message 字段是 Option<Message> 类型）
        ensure!(
            request.message.is_some(),
            "message content cannot be empty"
        );

        Ok(())
    }

    fn validate_notification_request(&self, request: &PushNotificationRequest) -> Result<()> {
        // 校验用户ID列表不为空
        ensure!(
            !request.user_ids.is_empty(),
            "user_ids cannot be empty"
        );

        // 校验用户ID数量限制
        ensure!(
            request.user_ids.len() <= 1000,
            "user_ids count exceeds maximum limit of 1000"
        );

        // 校验通知内容不为空（notification 字段是 Notification 类型）
        ensure!(
            request.notification.is_some(),
            "notification content cannot be empty"
        );

        // 校验通知标题不为空（如果提供了 notification）
        if let Some(notification) = &request.notification {
            ensure!(
                !notification.title.is_empty(),
                "notification title cannot be empty"
            );

            // 校验标题长度限制
            ensure!(
                notification.title.len() <= 256,
                "notification title exceeds maximum length of 256"
            );
        }

        Ok(())
    }
}

