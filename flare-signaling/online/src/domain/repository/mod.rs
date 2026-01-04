use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::aggregate::Connection;
use crate::domain::model::{DeviceInfo, OnlineStatusRecord};
use crate::domain::value_object::{DeviceId, ConnectionId, UserId};

// Rust 2024: 对于需要作为 trait 对象使用的 trait（Arc<dyn Trait>），
// 如果方法参数包含引用，需要保留 async-trait 宏

#[async_trait]
pub trait ConversationRepository: Send + Sync {
    async fn save_connection(&self, connection: &Connection) -> Result<()>;
    async fn remove_connection(&self, conversation_id: &ConnectionId, user_id: &UserId) -> Result<()>;
    async fn touch_connection(&self, user_id: &UserId) -> Result<()>;
    async fn fetch_statuses(
        &self,
        user_ids: &[String],
    ) -> Result<HashMap<String, OnlineStatusRecord>>;
    async fn get_user_connections(&self, user_id: &UserId) -> Result<Vec<Connection>>;
    async fn remove_user_connections(
        &self,
        user_id: &UserId,
        device_ids: Option<&[DeviceId]>,
    ) -> Result<()>;
    async fn get_connection_by_device(
        &self,
        user_id: &UserId,
        device_id: &DeviceId,
    ) -> Result<Option<Connection>>;
    async fn list_user_devices(&self, ctx: &flare_server_core::context::Context) -> Result<Vec<DeviceInfo>>;
    async fn get_device(&self, ctx: &flare_server_core::context::Context, device_id: &str) -> Result<Option<DeviceInfo>>;

    async fn list_user_connections(&self, ctx: &flare_server_core::context::Context) -> Result<Vec<Connection>> {
        let user_id = ctx.user_id().ok_or_else(|| anyhow::anyhow!("user_id is required in context"))?;
        let user_id_vo = UserId::new(user_id.to_string()).map_err(|e| anyhow::anyhow!(e))?;
        self.get_user_connections(&user_id_vo).await
    }
}

/// 订阅仓库接口
#[async_trait]
pub trait SubscriptionRepository: Send + Sync {
    /// 添加订阅
    async fn add_subscription(
        &self,
        user_id: &str,
        topic: &str,
        params: &HashMap<String, String>,
    ) -> Result<()>;
    async fn remove_subscription(&self, ctx: &flare_server_core::context::Context, topics: &[String]) -> Result<()>;
    async fn get_user_subscriptions(
        &self,
        ctx: &flare_server_core::context::Context,
    ) -> Result<Vec<(String, HashMap<String, String>)>>;
    /// 获取主题的所有订阅者
    async fn get_topic_subscribers(&self, topic: &str) -> Result<Vec<String>>;
}

/// 信号发布接口
#[async_trait]
pub trait SignalPublisher: Send + Sync {
    /// 发布信号到主题
    async fn publish_signal(
        &self,
        topic: &str,
        payload: &[u8],
        metadata: &HashMap<String, String>,
    ) -> Result<()>;
}

/// 在线状态监听接口

#[async_trait]
pub trait PresenceWatcher: Send + Sync {
    /// 监听用户在线状态变化
    async fn watch_presence(
        &self,
        user_ids: &[String],
    ) -> Result<tokio::sync::mpsc::Receiver<anyhow::Result<PresenceChangeEvent>>>;
}

/// 在线状态变化事件
#[derive(Debug, Clone)]
pub struct PresenceChangeEvent {
    pub user_id: String,
    pub status: OnlineStatusRecord,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub conflict_action: Option<i32>, // ConflictAction enum value
    pub reason: Option<String>,
}
