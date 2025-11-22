//! 请求校验基础设施层

pub mod request_validator;

pub use request_validator::RequestValidatorImpl;

use anyhow::Result;
use flare_proto::push::{PushMessageRequest, PushNotificationRequest};

/// 请求校验器 trait
pub trait RequestValidator: Send + Sync {
    /// 校验推送消息请求
    fn validate_message_request(&self, request: &PushMessageRequest) -> Result<()>;

    /// 校验推送通知请求
    fn validate_notification_request(&self, request: &PushNotificationRequest) -> Result<()>;
}

