//! 领域模型定义

use std::collections::HashMap;
use flare_proto::common::{MessageReadRecord, MessageOperation, VisibilityStatus};
use prost_types::Timestamp;

/// 消息更新结构
pub struct MessageUpdate {
    pub is_recalled: Option<bool>,
    pub recalled_at: Option<Timestamp>,
    pub visibility: Option<HashMap<String, VisibilityStatus>>,
    pub read_by: Option<Vec<MessageReadRecord>>,
    pub operations: Option<Vec<MessageOperation>>,
    pub attributes: Option<HashMap<String, String>>,
    pub tags: Option<Vec<String>>,
}

