//! 命令结构体定义（Command DTO）

use std::collections::HashMap;

/// 删除消息命令
#[derive(Debug, Clone)]
pub struct DeleteMessageCommand {
    pub message_ids: Vec<String>,
}

/// 撤回消息命令
#[derive(Debug, Clone)]
pub struct RecallMessageCommand {
    pub message_id: String,
    pub recall_time_limit_seconds: i64,
}

/// 清理会话命令
#[derive(Debug, Clone)]
pub struct ClearSessionCommand {
    pub session_id: String,
    pub user_id: Option<String>,
    pub clear_before_time: Option<chrono::DateTime<chrono::Utc>>,
}

/// 标记消息已读命令
#[derive(Debug, Clone)]
pub struct MarkReadCommand {
    pub message_id: String,
    pub user_id: String,
}

/// 为用户删除消息命令
#[derive(Debug, Clone)]
pub struct DeleteMessageForUserCommand {
    pub message_ids: Vec<String>,
    pub user_id: String,
    pub session_id: String,
    pub permanent: bool,
}

/// 设置消息属性命令
#[derive(Debug, Clone)]
pub struct SetMessageAttributesCommand {
    pub message_id: String,
    pub attributes: HashMap<String, String>,
    pub tags: Vec<String>,
}

/// 导出消息命令
#[derive(Debug, Clone)]
pub struct ExportMessagesCommand {
    pub session_id: String,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<i32>,
}
