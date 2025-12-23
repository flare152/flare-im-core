//! 仓储接口定义（Port）

use anyhow::Result;
use async_trait::async_trait;
use flare_proto::common::Message;

use crate::domain::events::AckEvent;
use crate::domain::model::MediaAttachmentMetadata;

// Rust 2024: trait 中直接使用 async fn（原生支持，包括 trait 对象）
#[async_trait]
pub trait MessageIdempotencyRepository: Send + Sync {
    /// 检查消息ID是否为新消息（基于服务端消息ID）
    async fn is_new(&self, message_id: &str) -> Result<bool>;
    
    /// 检查客户端消息ID是否为新消息（基于客户端消息ID，用于去重）
    /// 
    /// # 参数
    /// * `client_msg_id` - 客户端消息ID
    /// * `sender_id` - 发送者ID（可选，用于更精确的去重检查）
    /// 
    /// # 返回
    /// * `Ok(true)` - 是新消息
    /// * `Ok(false)` - 是重复消息
    async fn is_new_by_client_msg_id(&self, client_msg_id: &str, _sender_id: Option<&str>) -> Result<bool> {
        // 默认实现：如果client_msg_id为空，返回true（允许通过）
        if client_msg_id.is_empty() {
            return Ok(true);
        }
        // 默认实现：委托给is_new（子类可以覆盖以优化）
        self.is_new(client_msg_id).await
    }
}

#[async_trait]
pub trait HotCacheRepository: Send + Sync {
    async fn store_hot(&self, message: &Message) -> Result<()>;

    /// 批量存储消息（优化性能）
    async fn store_hot_batch(&self, messages: &[Message]) -> Result<()> {
        // 默认实现：逐个存储（子类可以覆盖以优化）
        for message in messages {
            self.store_hot(message).await?;
        }
        Ok(())
    }
}

#[async_trait]
pub trait RealtimeStoreRepository: Send + Sync {
    async fn store_realtime(&self, message: &Message) -> Result<()>;

    /// 批量存储消息（优化性能）
    async fn store_realtime_batch(&self, messages: &[Message]) -> Result<()> {
        // 默认实现：逐个存储（子类可以覆盖以优化）
        for message in messages {
            self.store_realtime(message).await?;
        }
        Ok(())
    }
}

#[async_trait]
pub trait ArchiveStoreRepository: Send + Sync {
    async fn store_archive(&self, message: &Message) -> Result<()>;

    /// 批量存储消息（优化性能）
    async fn store_archive_batch(&self, messages: &[Message]) -> Result<()> {
        // 默认实现：逐个存储（子类可以覆盖以优化）
        for message in messages {
            self.store_archive(message).await?;
        }
        Ok(())
    }

    /// 更新消息状态（用于撤回、编辑、删除等操作）
    async fn update_message_status(
        &self,
        message_id: &str,
        status: flare_proto::common::MessageStatus,
        is_recalled: Option<bool>,
        recalled_at: Option<prost_types::Timestamp>,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, status, is_recalled, recalled_at);
        Ok(())
    }

    /// 更新消息内容（用于编辑操作）
    async fn update_message_content(
        &self,
        message_id: &str,
        new_content: &flare_proto::common::MessageContent,
        edit_version: i32,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, new_content, edit_version);
        Ok(())
    }

    /// 更新消息可见性（用于删除操作）
    async fn update_message_visibility(
        &self,
        message_id: &str,
        user_id: Option<&str>,
        visibility: flare_proto::common::VisibilityStatus,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, user_id, visibility);
        Ok(())
    }

    /// 追加操作记录到消息
    async fn append_operation(
        &self,
        message_id: &str,
        operation: &flare_proto::common::MessageOperation,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, operation);
        Ok(())
    }

    /// 获取 Any trait 引用（用于向下转型）
    fn as_any(&self) -> &dyn std::any::Any;
}

#[async_trait]
pub trait WalCleanupRepository: Send + Sync {
    async fn remove(&self, message_id: &str) -> Result<()>;
}

#[async_trait]
pub trait ConversationStateRepository: Send + Sync {
    async fn apply_message(&self, message: &Message) -> Result<()>;
}

#[async_trait]
pub trait UserSyncCursorRepository: Send + Sync {
    async fn advance_cursor(&self, conversation_id: &str, user_id: &str, message_ts: i64) -> Result<()>;
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
pub trait ConversationRepository: Send + Sync {
    /// 确保 conversation 存在，如果不存在则创建
    async fn ensure_conversation(
        &self,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Result<()>;
}

/// Session 更新仓储接口 - 用于更新会话和参与者信息（seq、未读数等）
#[async_trait]
pub trait ConversationUpdateRepository: Send + Sync {
    /// 更新会话的最后消息信息
    async fn update_last_message(&self, conversation_id: &str, message_id: &str, seq: i64)
    -> Result<()>;

    /// 批量更新参与者的未读数
    async fn batch_update_unread_count(
        &self,
        conversation_id: &str,
        last_message_seq: i64,
        exclude_user_id: Option<&str>,
    ) -> Result<()>;
}

/// Seq 生成器接口
#[async_trait]
pub trait SeqGenerator: Send + Sync {
    /// 为指定会话生成下一个 seq
    async fn generate_seq(&self, conversation_id: &str) -> Result<i64>;
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
    async fn batch_check_deleted(
        &self,
        user_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<String>>;
}
