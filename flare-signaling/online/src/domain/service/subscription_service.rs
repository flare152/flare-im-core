//! 订阅领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;

use anyhow::Result;
use flare_proto::signaling::{PublishSignalRequest, PublishSignalResponse, SubscribeRequest, SubscribeResponse, Subscription, UnsubscribeRequest, UnsubscribeResponse};
use tracing::{info, warn};

use crate::domain::repository::{SignalPublisher, SubscriptionRepository};
use crate::util;

/// 订阅领域服务 - 包含所有业务逻辑
pub struct SubscriptionService {
    subscription_repo: Arc<dyn SubscriptionRepository + Send + Sync>,
    signal_publisher: Arc<dyn SignalPublisher + Send + Sync>,
}

impl SubscriptionService {
    pub fn new(
        subscription_repo: Arc<dyn SubscriptionRepository + Send + Sync>,
        signal_publisher: Arc<dyn SignalPublisher + Send + Sync>,
    ) -> Self {
        Self {
            subscription_repo,
            signal_publisher,
        }
    }

    /// 订阅频道/主题
    pub async fn subscribe(&self, request: SubscribeRequest) -> Result<SubscribeResponse> {
        let user_id = &request.user_id;
        let subscriptions = &request.subscriptions;

        if subscriptions.is_empty() {
            return Ok(SubscribeResponse {
                granted: vec![],
                status: util::rpc_status_error(
                    flare_server_core::error::ErrorCode::InvalidParameter,
                    "subscriptions list is empty",
                ),
            });
        }

        let mut granted = Vec::new();
        for subscription in subscriptions {
            let topic = &subscription.topic;
            
            if topic.is_empty() {
                warn!(user_id = %user_id, "empty topic in subscription request");
                continue;
            }

            // 添加订阅
            if let Err(err) = self
                .subscription_repo
                .add_subscription(user_id, topic, &subscription.params)
                .await
            {
                warn!(user_id = %user_id, topic = %topic, error = %err, "failed to add subscription");
                continue;
            }

            granted.push(Subscription {
                topic: topic.clone(),
                params: subscription.params.clone(),
            });

            info!(user_id = %user_id, topic = %topic, "subscription added");
        }

        Ok(SubscribeResponse {
            granted,
            status: util::rpc_status_ok(),
        })
    }

    /// 取消订阅
    pub async fn unsubscribe(&self, request: UnsubscribeRequest) -> Result<UnsubscribeResponse> {
        let user_id = &request.user_id;
        let topics = &request.topics;

        if topics.is_empty() {
            return Ok(UnsubscribeResponse {
                status: util::rpc_status_error(
                    flare_server_core::error::ErrorCode::InvalidParameter,
                    "topics list is empty",
                ),
            });
        }

        if let Err(err) = self.subscription_repo.remove_subscription(user_id, topics).await {
            warn!(user_id = %user_id, error = %err, "failed to remove subscriptions");
            return Ok(UnsubscribeResponse {
                status: util::rpc_status_error(
                    flare_server_core::error::ErrorCode::InternalError,
                    &format!("failed to unsubscribe: {}", err),
                ),
            });
        }

        info!(user_id = %user_id, topics = ?topics, "subscriptions removed");

        Ok(UnsubscribeResponse {
            status: util::rpc_status_ok(),
        })
    }

    /// 发布信令消息
    pub async fn publish_signal(&self, request: PublishSignalRequest) -> Result<PublishSignalResponse> {
        let envelope = request.envelope.as_ref().ok_or_else(|| {
            flare_server_core::error::ErrorBuilder::new(
                flare_server_core::error::ErrorCode::InvalidParameter,
                "envelope is required",
            )
            .build_error()
        })?;

        let topic = &envelope.topic;
        if topic.is_empty() {
            return Ok(PublishSignalResponse {
                status: util::rpc_status_error(
                    flare_server_core::error::ErrorCode::InvalidParameter,
                    "topic is required",
                ),
            });
        }

        // 获取主题的所有订阅者
        let subscribers = match self.subscription_repo.get_topic_subscribers(topic).await {
            Ok(subs) => subs,
            Err(err) => {
                warn!(topic = %topic, error = %err, "failed to get topic subscribers");
                return Ok(PublishSignalResponse {
                    status: util::rpc_status_error(
                        flare_server_core::error::ErrorCode::InternalError,
                        &format!("failed to get subscribers: {}", err),
                    ),
                });
            }
        };

        // 如果有指定的目标用户，只发布给这些用户
        let targets = if envelope.targets.is_empty() {
            subscribers
        } else {
            // 过滤出同时订阅了主题且在被指定目标列表中的用户
            envelope
                .targets
                .iter()
                .filter(|target| subscribers.contains(target))
                .cloned()
                .collect()
        };

        if targets.is_empty() {
            info!(topic = %topic, "no subscribers for topic");
        } else {
            // 发布信号
            if let Err(err) = self
                .signal_publisher
                .publish_signal(topic, &envelope.payload, &envelope.metadata)
                .await
            {
                warn!(topic = %topic, error = %err, "failed to publish signal");
                return Ok(PublishSignalResponse {
                    status: util::rpc_status_error(
                        flare_server_core::error::ErrorCode::InternalError,
                        &format!("failed to publish signal: {}", err),
                    ),
                });
            }

            info!(
                topic = %topic,
                subscribers = targets.len(),
                from = %envelope.from,
                "signal published"
            );
        }

        Ok(PublishSignalResponse {
            status: util::rpc_status_ok(),
        })
    }
}

