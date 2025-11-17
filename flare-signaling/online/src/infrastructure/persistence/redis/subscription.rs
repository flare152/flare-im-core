use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use serde_json::json;

use crate::config::OnlineConfig;
use crate::domain::repositories::SubscriptionRepository;

const SUBSCRIPTION_KEY_PREFIX: &str = "subscription";
const TOPIC_SUBSCRIBERS_KEY_PREFIX: &str = "topic:subscribers";

pub struct RedisSubscriptionRepository {
    client: Arc<redis::Client>,
    config: Arc<OnlineConfig>,
}

impl RedisSubscriptionRepository {
    pub fn new(client: Arc<redis::Client>, config: Arc<OnlineConfig>) -> Self {
        Self { client, config }
    }

    fn subscription_key(&self, user_id: &str) -> String {
        format!("{}:{}", SUBSCRIPTION_KEY_PREFIX, user_id)
    }

    fn topic_subscribers_key(&self, topic: &str) -> String {
        format!("{}:{}", TOPIC_SUBSCRIBERS_KEY_PREFIX, topic)
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        ConnectionManager::new(self.client.as_ref().clone())
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::ServiceUnavailable,
                    "failed to open redis connection",
                )
                .details(err.to_string())
                .build_error()
            })
    }
}

#[async_trait]
impl SubscriptionRepository for RedisSubscriptionRepository {
    async fn add_subscription(
        &self,
        user_id: &str,
        topic: &str,
        params: &HashMap<String, String>,
    ) -> Result<()> {
        let mut conn = self.connection().await?;
        let user_key = self.subscription_key(user_id);
        let topic_key = self.topic_subscribers_key(topic);

        // 保存用户的订阅
        let subscription_value = json!({
            "topic": topic,
            "params": params,
        });
        let _: () = conn
            .hset(&user_key, topic, subscription_value.to_string())
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to add subscription")
                    .details(err.to_string())
                    .build_error()
            })?;

        // 添加到主题的订阅者列表
        let _: i64 = conn
            .sadd(&topic_key, user_id)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::DatabaseError,
                    "failed to add topic subscriber",
                )
                .details(err.to_string())
                .build_error()
            })?;

        // 设置过期时间
        let _: bool = conn
            .expire(&user_key, self.config.redis_ttl_seconds as i64)
            .await
            .map_err(|err| {
                ErrorBuilder::new(ErrorCode::DatabaseError, "failed to set subscription ttl")
                    .details(err.to_string())
                    .build_error()
            })?;

        Ok(())
    }

    async fn remove_subscription(&self, user_id: &str, topics: &[String]) -> Result<()> {
        let mut conn = self.connection().await?;
        let user_key = self.subscription_key(user_id);

        for topic in topics {
            // 从用户的订阅中移除
            let _: i64 = conn
                .hdel(&user_key, topic)
                .await
                .map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::DatabaseError,
                        "failed to remove subscription",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;

            // 从主题的订阅者列表中移除
            let topic_key = self.topic_subscribers_key(topic);
            let _: i64 = conn
                .srem(&topic_key, user_id)
                .await
                .map_err(|err| {
                    ErrorBuilder::new(
                        ErrorCode::DatabaseError,
                        "failed to remove topic subscriber",
                    )
                    .details(err.to_string())
                    .build_error()
                })?;
        }

        Ok(())
    }

    async fn get_user_subscriptions(
        &self,
        user_id: &str,
    ) -> Result<Vec<(String, HashMap<String, String>)>> {
        let mut conn = self.connection().await?;
        let user_key = self.subscription_key(user_id);
        let subscriptions: HashMap<String, String> = conn
            .hgetall(&user_key)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::DatabaseError,
                    "failed to get user subscriptions",
                )
                .details(err.to_string())
                .build_error()
            })?;

        let mut result = Vec::new();
        for (topic, value_str) in subscriptions {
            let value: serde_json::Value = serde_json::from_str(&value_str).map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::DeserializationError,
                    "failed to parse subscription",
                )
                .details(err.to_string())
                .build_error()
            })?;

            let params: HashMap<String, String> = value
                .get("params")
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();

            result.push((topic, params));
        }

        Ok(result)
    }

    async fn get_topic_subscribers(&self, topic: &str) -> Result<Vec<String>> {
        let mut conn = self.connection().await?;
        let topic_key = self.topic_subscribers_key(topic);
        let subscribers: Vec<String> = conn
            .smembers(&topic_key)
            .await
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::DatabaseError,
                    "failed to get topic subscribers",
                )
                .details(err.to_string())
                .build_error()
            })?;

        Ok(subscribers)
    }
}

