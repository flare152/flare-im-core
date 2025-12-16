use async_trait::async_trait;
use flare_server_core::error::Result;
use std::collections::HashMap;

use crate::domain::model::PushDispatchTask;

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

    /// 查询某个会话（聊天室/群）的所有在线用户ID列表
    ///
    /// # 参数
    /// * `session_id` - 会话ID（聊天室ID或群ID）
    ///
    /// # 返回
    /// * `Ok(Vec<String>)` - 在线用户ID列表
    /// * `Err(Error)` - 查询失败
    ///
    /// # 注意
    /// 此方法用于聊天室消息推送场景，当业务系统未提供 receiver_ids 时，
    /// 自动查询该聊天室的所有在线用户进行推送。
    async fn get_all_online_users_for_session(&self, session_id: &str) -> Result<Vec<String>>;
}

#[async_trait]
pub trait PushTaskPublisher: Send + Sync {
    async fn publish(&self, task: &PushDispatchTask) -> Result<()>;

    /// 批量发布离线推送任务
    async fn publish_offline_batch(&self, tasks: &[PushDispatchTask]) -> Result<()>;

    /// 发布到死信队列
    async fn publish_to_dlq(
        &self,
        task: &PushDispatchTask,
        error: &str,
        retry_count: u32,
    ) -> Result<()>;
}
