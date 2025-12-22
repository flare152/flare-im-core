use std::collections::HashMap;

use crate::domain::model::{ConversationFilter, ConversationSort};

/// 列出会话查询
#[derive(Debug, Clone)]
pub struct ListConversationsQuery {
    pub user_id: String,
    pub cursor: Option<String>,
    pub limit: i32,
}

/// 搜索会话查询
#[derive(Debug, Clone)]
pub struct SearchConversationsQuery {
    pub user_id: Option<String>,
    pub filters: Vec<ConversationFilter>,
    pub sort: Vec<ConversationSort>,
    pub limit: usize,
    pub offset: usize,
}

/// 会话引导查询
#[derive(Debug, Clone)]
pub struct ConversationBootstrapQuery {
    pub user_id: String,
    pub client_cursor: HashMap<String, i64>,
    pub include_recent: bool,
    pub recent_limit: Option<i32>,
}

/// 同步消息查询
#[derive(Debug, Clone)]
pub struct SyncMessagesQuery {
    pub conversation_id: String,
    pub since_ts: i64,
    pub cursor: Option<String>,
    pub limit: i32,
}
