//! 命令结构体定义（Command DTO）

use flare_proto::push::{PushMessageRequest, PushNotificationRequest};

/// 入队推送消息命令
#[derive(Debug, Clone)]
pub struct EnqueueMessageCommand {
    /// 原始请求
    pub request: PushMessageRequest,
}

/// 入队推送通知命令
#[derive(Debug, Clone)]
pub struct EnqueueNotificationCommand {
    /// 原始请求
    pub request: PushNotificationRequest,
}
