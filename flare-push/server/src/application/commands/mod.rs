//! 命令结构体定义（Command DTO）

use flare_proto::push::{PushMessageRequest, PushNotificationRequest};

/// 推送消息命令
#[derive(Debug, Clone)]
pub struct PushMessageCommand {
    /// 原始请求
    pub request: PushMessageRequest,
}

/// 推送通知命令
#[derive(Debug, Clone)]
pub struct PushNotificationCommand {
    /// 原始请求
    pub request: PushNotificationRequest,
}

/// 批量推送任务命令
#[derive(Debug, Clone)]
pub struct BatchPushTasksCommand {
    /// 批量任务数据（序列化的 PushDispatchTask）
    pub batch: Vec<Vec<u8>>,
}
