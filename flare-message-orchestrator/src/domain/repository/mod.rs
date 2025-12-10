use std::sync::Arc;
use anyhow::Result;
use flare_proto::storage::StoreMessageRequest as StorageStoreMessageRequest;
use flare_proto::push::PushMessageRequest as PushPushMessageRequest;
use std::future::Future;
use std::pin::Pin;

use crate::domain::model::MessageSubmission;

/// 消息事件发布器（Rust 2024: 原生异步 trait）
pub trait MessageEventPublisher: Send + Sync {
    /// 发布消息到存储队列 (flare.im.message.created)
    fn publish_storage(&self, payload: StorageStoreMessageRequest) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    
    /// 发布推送任务到推送队列 (flare.im.push.tasks)
    fn publish_push(&self, payload: PushPushMessageRequest) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    
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
    fn publish_storage(&self, payload: StorageStoreMessageRequest) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => publisher.publish_storage(payload).await,
            }
        })
    }

    fn publish_push(&self, payload: PushPushMessageRequest) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            match self {
                MessageEventPublisherItem::Kafka(publisher) => publisher.publish_push(payload).await,
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
                MessageEventPublisherItem::Kafka(publisher) => publisher.publish_both(storage_payload, push_payload).await,
            }
        })
    }
}

/// WAL 仓储接口（Rust 2024: 原生异步 trait）
pub trait WalRepository: Send + Sync {
    fn append<'a>(&'a self, submission: &'a MessageSubmission) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

/// WalRepository 的枚举封装，用于在 Rust 2024 下避免 `dyn` + async trait 带来的
/// `E0038: trait is not dyn compatible` 问题。
#[derive(Debug)]
pub enum WalRepositoryItem {
    Noop(Arc<crate::infrastructure::persistence::noop_wal::NoopWalRepository>),
    Redis(Arc<crate::infrastructure::persistence::redis_wal::RedisWalRepository>),
}

impl WalRepository for WalRepositoryItem {
    fn append<'a>(&'a self, submission: &'a MessageSubmission) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        match self {
            WalRepositoryItem::Noop(repo) => Box::pin(repo.append(submission)),
            WalRepositoryItem::Redis(repo) => Box::pin(repo.append(submission)),
        }
    }
}

/// Session 仓储接口 - 用于确保 session 存在（Rust 2024: 原生异步 trait）
pub trait SessionRepository: Send + Sync {
    /// 确保 session 存在，如果不存在则创建
    fn ensure_session(
        &self,
        session_id: &str,
        session_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}

/// SessionRepository 的枚举封装，用于在 Rust 2024 下避免 `dyn` + async trait 带来的
/// `E0038: trait is not dyn compatible` 问题。
#[derive(Debug)]
pub enum SessionRepositoryItem {
    Grpc(Arc<crate::infrastructure::external::session_client::GrpcSessionClient>),
}

impl SessionRepository for SessionRepositoryItem {
    fn ensure_session(
        &self,
        session_id: &str,
        session_type: &str,
        business_type: &str,
        participants: Vec<String>,
        tenant_id: Option<&str>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        match self {
            SessionRepositoryItem::Grpc(repo) => repo.ensure_session(session_id, session_type, business_type, participants, tenant_id),
        }
    }
}