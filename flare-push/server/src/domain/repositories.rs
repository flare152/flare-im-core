use async_trait::async_trait;
use flare_server_core::error::Result;
use std::collections::HashMap;

use crate::domain::models::PushDispatchTask;

/// 用户在线状态信息
#[derive(Debug, Clone)]
pub struct OnlineStatus {
    pub user_id: String,
    pub online: bool,
    pub gateway_id: Option<String>,
    pub server_id: Option<String>,
}

#[async_trait]
pub trait OnlineStatusRepository: Send + Sync {
    /// 查询单个用户在线状态
    async fn is_online(&self, user_id: &str) -> Result<bool>;
    
    /// 批量查询用户在线状态（返回用户ID到在线状态的映射）
    async fn batch_get_online_status(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatus>>;
    
    /// 查询所有在线用户（用于聊天室消息广播）
    /// 
    /// # 参数
    /// - `tenant_id`: 租户ID，如果为 None，查询所有租户的在线用户
    /// 
    /// # 返回
    /// 返回所有在线用户的 OnlineStatus 映射
    async fn get_all_online_users(
        &self,
        tenant_id: Option<&str>,
    ) -> Result<HashMap<String, OnlineStatus>>;
}

#[async_trait]
pub trait PushTaskPublisher: Send + Sync {
    async fn publish(&self, task: &PushDispatchTask) -> Result<()>;
    
    /// 批量发布离线推送任务
    async fn publish_offline_batch(&self, tasks: &[PushDispatchTask]) -> Result<()>;
    
    /// 发布到死信队列
    async fn publish_to_dlq(&self, task: &PushDispatchTask, error: &str, retry_count: u32) -> Result<()>;
}
