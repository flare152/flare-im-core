//! 消息操作命令（Command）
//!
//! 根据 CQRS 架构，Command 用于修改状态，不返回查询结果

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 消息操作命令基类
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageOperationCommand {
    /// 目标消息ID
    pub message_id: String,
    /// 操作者ID
    pub operator_id: String,
    /// 操作时间戳
    pub timestamp: DateTime<Utc>,
    /// 租户ID
    pub tenant_id: String,
    /// 会话ID
    pub conversation_id: String,
}

/// 撤回消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 撤回原因（可选）
    pub reason: Option<String>,
    /// 撤回时间限制（秒，可选）
    pub time_limit_seconds: Option<i32>,
}

/// 编辑消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 编辑后的内容（二进制）
    pub new_content: Vec<u8>,
    /// 编辑原因（可选）
    pub reason: Option<String>,
}

/// 删除消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 删除类型（软删除/硬删除）
    pub delete_type: DeleteType,
    /// 删除原因（可选）
    pub reason: Option<String>,
    /// 目标用户ID（软删除时使用，硬删除时为空）
    pub target_user_id: Option<String>,
    /// 要删除的消息ID列表（批量删除）
    pub message_ids: Vec<String>,
    /// 是否通知其他用户
    pub notify_others: bool,
}

/// 删除类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeleteType {
    /// 软删除（仅对当前用户隐藏）
    Soft,
    /// 硬删除（永久删除，仅管理员）
    Hard,
}

/// 已读消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 已读的消息ID列表（批量已读）
    pub message_ids: Vec<String>,
    /// 已读时间戳（可选，默认当前时间）
    pub read_at: Option<DateTime<Utc>>,
    /// 是否阅后即焚
    pub burn_after_read: bool,
}

/// 添加反应命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddReactionCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 表情符号
    pub emoji: String,
}

/// 移除反应命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveReactionCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 表情符号
    pub emoji: String,
}

/// 置顶消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 置顶原因（可选）
    pub reason: Option<String>,
    /// 置顶到期时间（可选）
    pub expire_at: Option<DateTime<Utc>>,
}

/// 取消置顶消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnpinMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
}

/// 标记消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 标记类型（0: Important, 1: Todo, 2: Done, 3: Custom）
    pub mark_type: i32,
    /// 标记值（可选）
    pub mark_value: Option<String>,
}

/// 取消标记消息命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnmarkMessageCommand {
    /// 基础命令字段
    #[serde(flatten)]
    pub base: MessageOperationCommand,
    /// 标记类型（可选，如果为 None 则取消所有标记）
    pub mark_type: Option<i32>,
    /// 用户ID（执行取消标记操作的用户）
    pub user_id: String,
}

/// 批量标记已读命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchMarkMessageReadCommand {
    /// 会话ID
    pub conversation_id: String,
    /// 用户ID
    pub user_id: String,
    /// 要标记已读的消息ID列表（如果为空，则标记会话中所有未读消息）
    pub message_ids: Vec<String>,
    /// 已读时间戳（可选，默认当前时间）
    pub read_at: Option<DateTime<Utc>>,
    /// 租户ID
    pub tenant_id: String,
}

/// 标记会话已读命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkConversationReadCommand {
    /// 会话ID
    pub conversation_id: String,
    /// 用户ID
    pub user_id: String,
    /// 已读时间戳（可选，默认当前时间）
    pub read_at: Option<DateTime<Utc>>,
    /// 租户ID
    pub tenant_id: String,
}

/// 标记全部会话已读命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkAllConversationsReadCommand {
    /// 用户ID
    pub user_id: String,
    /// 已读时间戳（可选，默认当前时间）
    pub read_at: Option<DateTime<Utc>>,
    /// 会话类型筛选（可选，如果为空则标记所有类型的会话）
    pub conversation_types: Vec<String>,
    /// 租户ID
    pub tenant_id: String,
}

/// 处理临时消息命令（只推送，不持久化）
#[derive(Debug, Clone)]
pub struct HandleTemporaryMessageCommand {
    /// 消息（proto 类型）
    pub message: flare_proto::common::Message,
}

