//! 订阅管理服务
//!
//! 负责管理用户对主题的订阅和取消订阅

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use flare_server_core::error::Result;

/// 订阅信息
#[derive(Debug, Clone)]
pub struct SubscriptionInfo {
    pub user_id: String,
    pub topic: String,
    pub params: HashMap<String, String>,
    pub subscribed_at: chrono::DateTime<chrono::Utc>,
}

/// 订阅管理服务
pub struct SubscriptionService {
    /// 用户订阅映射：user_id -> topic -> SubscriptionInfo
    user_subscriptions: Arc<RwLock<HashMap<String, HashMap<String, SubscriptionInfo>>>>,
    /// 主题订阅映射：topic -> user_ids
    topic_subscribers: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl SubscriptionService {
    pub fn new() -> Self {
        Self {
            user_subscriptions: Arc::new(RwLock::new(HashMap::new())),
            topic_subscribers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 订阅主题
    pub async fn subscribe(
        &self,
        user_id: &str,
        subscriptions: &[flare_proto::access_gateway::Subscription],
    ) -> Result<Vec<flare_proto::access_gateway::Subscription>> {
        let mut granted_subscriptions = Vec::new();

        // 获取写锁
        let mut user_subs = self.user_subscriptions.write().await;
        let mut topic_subs = self.topic_subscribers.write().await;

        // 为每个订阅项处理
        for subscription in subscriptions {
            let topic = &subscription.topic;

            // 更新用户订阅映射
            let user_topics = user_subs
                .entry(user_id.to_string())
                .or_insert_with(HashMap::new);
            let subscription_info = SubscriptionInfo {
                user_id: user_id.to_string(),
                topic: topic.clone(),
                params: subscription.params.clone(),
                subscribed_at: chrono::Utc::now(),
            };
            user_topics.insert(topic.clone(), subscription_info);

            // 更新主题订阅者映射
            let subscribers = topic_subs.entry(topic.clone()).or_insert_with(Vec::new);
            if !subscribers.contains(&user_id.to_string()) {
                subscribers.push(user_id.to_string());
            }

            // 添加到已授权的订阅列表
            granted_subscriptions.push(subscription.clone());

            info!(
                user_id = %user_id,
                topic = %topic,
                "User subscribed to topic"
            );
        }

        // 释放写锁
        drop(user_subs);
        drop(topic_subs);

        debug!(
            user_id = %user_id,
            count = granted_subscriptions.len(),
            "Subscribed to {} topics",
            granted_subscriptions.len()
        );

        Ok(granted_subscriptions)
    }

    /// 取消订阅主题
    pub async fn unsubscribe(&self, user_id: &str, topics: &[String]) -> Result<()> {
        // 获取写锁
        let mut user_subs = self.user_subscriptions.write().await;
        let mut topic_subs = self.topic_subscribers.write().await;

        // 为每个主题处理取消订阅
        for topic in topics {
            // 从用户订阅映射中移除
            if let Some(user_topics) = user_subs.get_mut(user_id) {
                user_topics.remove(topic);
                // 如果用户没有任何订阅了，清理用户条目
                if user_topics.is_empty() {
                    user_subs.remove(user_id);
                }
            }

            // 从主题订阅者映射中移除用户
            if let Some(subscribers) = topic_subs.get_mut(topic) {
                subscribers.retain(|subscriber_id| subscriber_id != user_id);
                // 如果主题没有任何订阅者了，清理主题条目
                if subscribers.is_empty() {
                    topic_subs.remove(topic);
                }
            }

            info!(
                user_id = %user_id,
                topic = %topic,
                "User unsubscribed from topic"
            );
        }

        // 释放写锁
        drop(user_subs);
        drop(topic_subs);

        debug!(
            user_id = %user_id,
            count = topics.len(),
            "Unsubscribed from {} topics",
            topics.len()
        );

        Ok(())
    }

    /// 获取用户的所有订阅
    pub async fn get_user_subscriptions(&self, user_id: &str) -> Result<Vec<SubscriptionInfo>> {
        let user_subs = self.user_subscriptions.read().await;

        let subscriptions = if let Some(user_topics) = user_subs.get(user_id) {
            user_topics.values().cloned().collect()
        } else {
            Vec::new()
        };

        Ok(subscriptions)
    }

    /// 获取订阅了特定主题的所有用户
    pub async fn get_topic_subscribers(&self, topic: &str) -> Result<Vec<String>> {
        let topic_subs = self.topic_subscribers.read().await;

        let subscribers = if let Some(subscribers) = topic_subs.get(topic) {
            subscribers.clone()
        } else {
            Vec::new()
        };

        Ok(subscribers)
    }

    /// 检查用户是否订阅了特定主题
    pub async fn is_subscribed(&self, user_id: &str, topic: &str) -> bool {
        let user_subs = self.user_subscriptions.read().await;

        if let Some(user_topics) = user_subs.get(user_id) {
            user_topics.contains_key(topic)
        } else {
            false
        }
    }
}

impl Default for SubscriptionService {
    fn default() -> Self {
        Self::new()
    }
}
