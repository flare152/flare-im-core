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

    /// 更新消息 FSM 状态（用于撤回、编辑、删除等操作）
    async fn update_message_fsm_state(
        &self,
        message_id: &str,
        fsm_state: &str,
        recall_reason: Option<&str>,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, fsm_state, recall_reason);
        Ok(())
    }

    /// 更新消息内容（用于编辑操作）
    async fn update_message_content(
        &self,
        message_id: &str,
        new_content: &flare_proto::common::MessageContent,
        edit_version: i32,
        editor_id: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, new_content, edit_version, editor_id, reason);
        Ok(())
    }

    /// 更新消息可见性（用于软删除操作）
    async fn update_message_visibility(
        &self,
        message_id: &str,
        user_id: &str,
        visibility_status: &str,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, user_id, visibility_status);
        Ok(())
    }

    /// 记录消息已读
    async fn record_message_read(
        &self,
        message_id: &str,
        user_id: &str,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, user_id);
        Ok(())
    }

    /// 添加或更新消息反应
    async fn upsert_message_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        user_id: &str,
        add: bool,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, emoji, user_id, add);
        Ok(())
    }

    /// 置顶或取消置顶消息
    async fn pin_message(
        &self,
        message_id: &str,
        conversation_id: &str,
        user_id: &str,
        pin: bool,
        expire_at: Option<chrono::DateTime<chrono::Utc>>,
        reason: Option<&str>,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, conversation_id, user_id, pin, expire_at, reason);
        Ok(())
    }

    /// 标记或取消标记消息
    async fn mark_message(
        &self,
        message_id: &str,
        conversation_id: &str,
        user_id: &str,
        mark_type: &str,
        color: Option<&str>,
        add: bool,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, conversation_id, user_id, mark_type, color, add);
        Ok(())
    }

    /// 追加操作记录到消息操作历史表
    async fn append_operation(
        &self,
        message_id: &str,
        operation: &flare_proto::common::MessageOperation,
    ) -> Result<()> {
        // 默认实现：空操作（子类必须实现）
        let _ = (message_id, operation);
        Ok(())
    }

    /// 获取消息（用于权限验证，写模型内部查询）
    ///
    /// 注意：这是写模型内部查询，符合 CQRS 原则（写操作可以在写模型内部查询）
    async fn get_message(&self, message_id: &str) -> Result<Option<Message>> {
        // 默认实现：返回 None（子类必须实现）
        let _ = message_id;
        Ok(None)
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
    async fn advance_cursor(&self, ctx: &flare_server_core::context::Context, conversation_id: &str, message_ts: i64) -> Result<()>;
}

#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish(&self, event: AckEvent<'_>) -> Result<()>;
}

#[async_trait]
pub trait MediaAttachmentVerifier: Send + Sync {
    async fn fetch_metadata(&self, ctx: &flare_server_core::context::Context, file_ids: &[String]) -> Result<Vec<MediaAttachmentMetadata>>;
}

/// Session 仓储接口 - 用于检查并创建 session
#[async_trait]
pub trait ConversationRepository: Send + Sync {
    /// 确保 conversation 存在，如果不存在则创建
    async fn ensure_conversation(
        &self,
        ctx: &flare_server_core::context::Context,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
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

// SeqGenerator 已移至编排服务（MessageOrchestrator）
// seq 在消息编排时生成，Writer 服务只负责持久化

