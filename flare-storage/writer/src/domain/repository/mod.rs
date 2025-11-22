//! 仓储接口定义（Port）

use anyhow::Result;
use async_trait::async_trait;
use flare_proto::common::Message;

use crate::domain::events::AckEvent;
use crate::domain::model::MediaAttachmentMetadata;

// Rust 2024: trait 中直接使用 async fn
// 注意：对于 trait 对象（dyn Trait），仍需要使用 async_trait
#[async_trait]
pub trait MessageIdempotencyRepository: Send + Sync {
    async fn is_new(&self, message_id: &str) -> Result<bool>;
}

#[async_trait]
pub trait HotCacheRepository: Send + Sync {
    async fn store_hot(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait RealtimeStoreRepository: Send + Sync {
    async fn store_realtime(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait ArchiveStoreRepository: Send + Sync {
    async fn store_archive(&self, message: &Message) -> Result<()>;
    
    /// 获取 Any trait 引用（用于向下转型）
    fn as_any(&self) -> &dyn std::any::Any;
}

#[async_trait]
pub trait WalCleanupRepository: Send + Sync {
    async fn remove(&self, message_id: &str) -> Result<()>;
}

#[async_trait]
pub trait SessionStateRepository: Send + Sync {
    async fn apply_message(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait UserSyncCursorRepository: Send + Sync {
    async fn advance_cursor(&self, session_id: &str, user_id: &str, message_ts: i64) -> Result<()>;
}

#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish(&self, event: AckEvent<'_>) -> Result<()>;
}

#[async_trait]
pub trait MediaAttachmentVerifier: Send + Sync {
    async fn fetch_metadata(&self, file_ids: &[String]) -> Result<Vec<MediaAttachmentMetadata>>;
}

/// Session 仓储接口 - 用于检查并创建 session
#[async_trait]
pub trait SessionRepository: Send + Sync {
    /// 确保 session 存在，如果不存在则创建
    async fn ensure_session(
        &self,
        session_id: &str,
        session_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Result<()>;
}

/// Session 更新仓储接口 - 用于更新会话和参与者信息（seq、未读数等）
#[async_trait]
pub trait SessionUpdateRepository: Send + Sync {
    /// 更新会话的最后消息信息
    async fn update_last_message(
        &self,
        session_id: &str,
        message_id: &str,
        seq: i64,
    ) -> Result<()>;

    /// 批量更新参与者的未读数
    async fn batch_update_unread_count(
        &self,
        session_id: &str,
        last_message_seq: i64,
        exclude_user_id: Option<&str>,
    ) -> Result<()>;
}

/// Seq 生成器接口
#[async_trait]
pub trait SeqGenerator: Send + Sync {
    /// 为指定会话生成下一个 seq
    async fn generate_seq(&self, session_id: &str) -> Result<i64>;
}

/// 消息状态仓储接口 - 用于存储和查询用户对消息的私有行为
#[async_trait]
pub trait MessageStateRepository: Send + Sync {
    /// 标记消息为已读
    async fn mark_as_read(&self, message_id: &str, user_id: &str) -> Result<()>;

    /// 标记消息为已删除（仅我删除）
    async fn mark_as_deleted(&self, message_id: &str, user_id: &str) -> Result<()>;

    /// 标记消息为已焚毁（阅后即焚）
    async fn mark_as_burned(&self, message_id: &str, user_id: &str) -> Result<()>;

    /// 检查消息是否已读
    async fn is_read(&self, message_id: &str, user_id: &str) -> Result<bool>;

    /// 检查消息是否已删除
    async fn is_deleted(&self, message_id: &str, user_id: &str) -> Result<bool>;

    /// 检查消息是否已焚毁
    async fn is_burned(&self, message_id: &str, user_id: &str) -> Result<bool>;

    /// 批量检查消息是否已删除（用于消息查询时过滤）
    async fn batch_check_deleted(&self, user_id: &str, message_ids: &[String]) -> Result<Vec<String>>;
}

