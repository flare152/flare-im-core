use anyhow::Result;
use async_trait::async_trait;
use flare_proto::storage::StoreMessageRequest;
use flare_proto::push::PushMessageRequest;

use crate::domain::model::MessageSubmission;

#[async_trait]
pub trait MessageEventPublisher {
    /// 发布消息到存储队列 (flare.im.message.created)
    async fn publish_storage(&self, payload: StoreMessageRequest) -> Result<()>;
    
    /// 发布推送任务到推送队列 (flare.im.push.tasks)
    async fn publish_push(&self, payload: PushMessageRequest) -> Result<()>;
    
    /// 并行发布到存储队列和推送队列（仅普通消息）
    async fn publish_both(
        &self,
        storage_payload: StoreMessageRequest,
        push_payload: PushMessageRequest,
    ) -> Result<()>;
}

#[async_trait]
pub trait WalRepository {
    async fn append(&self, submission: &MessageSubmission) -> Result<()>;
}

/// Session 仓储接口 - 用于确保 session 存在
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
