use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use flare_proto::common::Message;

use crate::domain::model::{
    ConflictResolutionPolicy, DevicePresence, DeviceState, MessageSyncResult, Conversation,
    ConversationBootstrapResult, ConversationParticipant, ConversationSummary,
};

#[derive(Clone, Debug)]
pub struct PresenceUpdate {
    pub user_id: String,
    pub device_id: String,
    pub device_platform: Option<String>,
    pub state: DeviceState,
    pub conflict_resolution: Option<ConflictResolutionPolicy>,
    pub notify_conflict: bool,
    pub conflict_reason: Option<String>,
}

/// Conversation 仓储接口（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn load_bootstrap(
        &self,
        user_id: &str,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<ConversationBootstrapResult>;

    async fn update_cursor(&self, user_id: &str, conversation_id: &str, ts: i64) -> Result<()>;

    // 会话管理方法
    async fn create_conversation(&self, conversation: &Conversation) -> Result<()>;
    async fn get_conversation(&self, conversation_id: &str) -> Result<Option<Conversation>>;
    async fn update_conversation(&self, conversation: &Conversation) -> Result<()>;
    async fn delete_conversation(&self, conversation_id: &str, hard_delete: bool) -> Result<()>;
    async fn manage_participants(
        &self,
        conversation_id: &str,
        to_add: &[ConversationParticipant],
        to_remove: &[String],
        role_updates: &[(String, Vec<String>)],
    ) -> Result<Vec<ConversationParticipant>>;
    async fn batch_acknowledge(&self, user_id: &str, cursors: &[(String, i64)]) -> Result<()>;
    async fn search_conversations(
        &self,
        user_id: Option<&str>,
        filters: &[crate::domain::model::ConversationFilter],
        sort: &[crate::domain::model::ConversationSort],
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<ConversationSummary>, usize)>;

    /// 标记消息为已读（更新 last_read_msg_seq）
    async fn mark_as_read(&self, user_id: &str, conversation_id: &str, seq: i64) -> Result<()>;

    /// 获取未读数（基于 last_message_seq - last_read_msg_seq）
    async fn get_unread_count(&self, user_id: &str, conversation_id: &str) -> Result<i32>;
}

/// Presence 仓储接口（需要作为 trait 对象使用，保留 async-trait）
#[async_trait]
pub trait PresenceRepository: Send + Sync {
    async fn list_devices(&self, user_id: &str) -> Result<Vec<DevicePresence>>;
    async fn update_presence(&self, update: PresenceUpdate) -> Result<()>;
}

#[async_trait]
pub trait MessageProvider: Send + Sync {
    async fn sync_messages(
        &self,
        conversation_id: &str,
        since_ts: i64,
        cursor: Option<&str>,
        limit: i32,
    ) -> Result<MessageSyncResult>;

    async fn recent_messages(
        &self,
        conversation_ids: &[String],
        limit_per_session: i32,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<Vec<Message>>;

    /// 基于 seq 的消息同步（可选，用于优化性能）
    ///
    /// # 参数
    /// * `conversation_id` - 会话ID
    /// * `after_seq` - 起始 seq（不包含）
    /// * `before_seq` - 结束 seq（可选，不包含）
    /// * `limit` - 返回消息数量限制
    ///
    /// # 返回
    /// * `Ok(MessageSyncResult)` - 消息同步结果，包含消息列表和游标
    ///
    /// # 注意
    /// 如果实现不支持基于 seq 的同步，可以返回 `Err` 或降级到基于时间戳的同步
    async fn sync_messages_by_seq(
        &self,
        _conversation_id: &str,
        _after_seq: i64,
        _before_seq: Option<i64>,
        _limit: i32,
    ) -> Result<MessageSyncResult> {
        // 默认实现：返回错误，提示使用基于时间戳的同步
        Err(anyhow::anyhow!(
            "sync_messages_by_seq not implemented, use sync_messages instead"
        ))
    }
}

/// Thread 仓储接口（话题管理）
#[async_trait]
pub trait ThreadRepository: Send + Sync {
    /// 创建话题
    async fn create_thread(
        &self,
        conversation_id: &str,
        root_message_id: &str,
        title: Option<&str>,
        creator_id: &str,
    ) -> Result<String>;

    /// 获取话题列表
    async fn list_threads(
        &self,
        conversation_id: &str,
        limit: i32,
        offset: i32,
        include_archived: bool,
        sort_order: crate::domain::model::ThreadSortOrder,
    ) -> Result<(Vec<crate::domain::model::Thread>, i32)>;

    /// 获取话题详情
    async fn get_thread(&self, thread_id: &str) -> Result<Option<crate::domain::model::Thread>>;

    /// 更新话题
    async fn update_thread(
        &self,
        thread_id: &str,
        title: Option<&str>,
        is_pinned: Option<bool>,
        is_locked: Option<bool>,
        is_archived: Option<bool>,
    ) -> Result<()>;

    /// 删除话题
    async fn delete_thread(&self, thread_id: &str) -> Result<()>;

    /// 增加话题回复计数
    async fn increment_reply_count(
        &self,
        thread_id: &str,
        reply_message_id: &str,
        reply_user_id: &str,
    ) -> Result<()>;

    /// 添加话题参与者
    async fn add_participant(&self, thread_id: &str, user_id: &str) -> Result<()>;

    /// 获取话题参与者列表
    async fn get_participants(&self, thread_id: &str) -> Result<Vec<String>>;
}
