//! 领域模型定义

use flare_proto::common::{MessageOperation, MessageReadRecord, Reaction, VisibilityStatus};
use prost_types::Timestamp;
use std::collections::HashMap;

/// 消息更新结构
#[derive(Default)]
pub struct MessageUpdate {
    pub is_recalled: Option<bool>,
    pub recalled_at: Option<Timestamp>,
    pub visibility: Option<HashMap<String, VisibilityStatus>>,
    pub read_by: Option<Vec<MessageReadRecord>>,
    pub operations: Option<Vec<MessageOperation>>,
    pub attributes: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
    pub reactions: Option<Vec<Reaction>>, // 反应列表
    /// 消息状态（可选，用于更新消息状态）
    pub status: Option<i32>, // MessageStatus 枚举值
}
