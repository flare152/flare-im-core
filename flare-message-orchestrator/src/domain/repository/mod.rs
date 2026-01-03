use anyhow::Result;
use flare_proto::push::PushMessageRequest as PushPushMessageRequest;
use flare_proto::storage::StoreMessageRequest as StorageStoreMessageRequest;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::domain::model::MessageSubmission;

/// 消息事件发布器（Rust 2024: 原生异步 trait）
pub trait MessageEventPublisher: Send + Sync {
    /// 发布消息到存储队列 (flare.im.message.created)
    fn publish_storage(
        &self,
        payload: StorageStoreMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// 发布操作消息到操作队列 (storage-message-operations)
    fn publish_operation(
        &self,
        payload: StorageStoreMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// 发布推送任务到推送队列 (flare.im.push.tasks)
    fn publish_push(
        &self,
        payload: PushPushMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;

    /// 并行发布到存储队列和推送队列（仅普通消息）
    fn publish_both(
        &self,
        storage_payload: StorageStoreMessageRequest,
        push_payload: PushPushMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// MessageEventPublisher 的枚举封装，用于在 Rust 2024 下避免 `dyn` + async trait 带来的
/// `E0038: trait is not dyn compatible` 问题。
pub enum MessageEventPublisherItem {
    Kafka(Arc<crate::infrastructure::messaging::kafka_publisher::KafkaMessagePublisher>),
}

impl std::fmt::Debug for MessageEventPublisherItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessageEventPublisherItem::Kafka(_) => f.debug_tuple("Kafka").finish(),
        }
    }
}

impl MessageEventPublisher for MessageEventPublisherItem {
    fn publish_storage(
        &self,
        payload: StorageStoreMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => {
                    publisher.publish_storage(payload).await
                }
            }
        })
    }

    fn publish_operation(
        &self,
        payload: StorageStoreMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => {
                    publisher.publish_operation(payload).await
                }
            }
        })
    }

    fn publish_push(
        &self,
        payload: PushPushMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => {
                    publisher.publish_push(payload).await
                }
            }
        })
    }

    fn publish_both(
        &self,
        storage_payload: StorageStoreMessageRequest,
        push_payload: PushPushMessageRequest,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => {
                    publisher.publish_both(storage_payload, push_payload).await
                }
            }
        })
    }
}

/// WAL 仓储接口（Rust 2024: 原生异步 trait）
pub trait WalRepository: Send + Sync {
    fn append<'a>(
        &'a self,
        submission: &'a MessageSubmission,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;

    /// 根据消息ID从 WAL 中查询消息（用于权限验证时的 fallback）
    fn find_by_message_id<'a>(
        &'a self,
        message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<flare_proto::common::Message>>> + Send + 'a>>;
}

/// WalRepository 的枚举封装，用于在 Rust 2024 下避免 `dyn` + async trait 带来的
/// `E0038: trait is not dyn compatible` 问题。
#[derive(Debug)]
pub enum WalRepositoryItem {
    Noop(Arc<crate::infrastructure::persistence::noop_wal::NoopWalRepository>),
    Redis(Arc<crate::infrastructure::persistence::redis_wal::RedisWalRepository>),
}

impl WalRepository for WalRepositoryItem {
    fn append<'a>(
        &'a self,
        submission: &'a MessageSubmission,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        match self {
            WalRepositoryItem::Noop(repo) => Box::pin(repo.append(submission)),
            WalRepositoryItem::Redis(repo) => Box::pin(repo.append(submission)),
        }
    }

    fn find_by_message_id<'a>(
        &'a self,
        message_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Option<flare_proto::common::Message>>> + Send + 'a>> {
        match self {
            WalRepositoryItem::Noop(repo) => Box::pin(repo.find_by_message_id(message_id)),
            WalRepositoryItem::Redis(repo) => Box::pin(repo.find_by_message_id(message_id)),
        }
    }
}

/// Conversation 仓储接口 - 用于确保 conversation 存在（Rust 2024: 原生异步 trait）
pub trait ConversationRepository: Send + Sync {
    /// 确保 conversation 存在，如果不存在则创建
    fn ensure_conversation(
        &self,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// ConversationRepository 的枚举封装，用于在 Rust 2024 下避免 `dyn` + async trait 带来的
/// `E0038: trait is not dyn compatible` 问题。
#[derive(Debug)]
pub enum ConversationRepositoryItem {
    Grpc(Arc<crate::infrastructure::external::session_client::GrpcConversationClient>),
}

impl ConversationRepository for ConversationRepositoryItem {
    fn ensure_conversation(
        &self,
        conversation_id: &str,
        conversation_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        match self {
            ConversationRepositoryItem::Grpc(repo) => repo.ensure_conversation(
                conversation_id,
                conversation_type,
                business_type,
                participants,
                tenant_id,
            ),
        }
    }
}
