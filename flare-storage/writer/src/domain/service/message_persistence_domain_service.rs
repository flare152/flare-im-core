//! 消息持久化领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;
use anyhow::{Result, anyhow};
use flare_im_core::utils::{current_millis, extract_timeline_from_extra};
use flare_proto::common::Message;
use flare_proto::storage::StoreMessageRequest;
use serde_json;
use tracing::{warn, instrument};
use uuid::Uuid;

use crate::domain::events::{AckEvent, AckStatus};
use crate::domain::model::{PersistenceResult, PreparedMessage};
use crate::domain::repository::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, SessionStateRepository,
    UserSyncCursorRepository, WalCleanupRepository,
};

/// 消息持久化领域服务 - 包含所有业务逻辑
/// 
/// 注意：领域服务不依赖基础设施层的监控指标，指标记录由应用层（Handler）负责
pub struct MessagePersistenceDomainService {
    idempotency_repo: Option<Arc<dyn MessageIdempotencyRepository + Send + Sync>>,
    hot_cache_repo: Option<Arc<dyn HotCacheRepository + Send + Sync>>,
    realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>>,
    archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
    wal_cleanup_repo: Option<Arc<dyn WalCleanupRepository + Send + Sync>>,
    ack_publisher: Option<Arc<dyn AckPublisher + Send + Sync>>,
    media_verifier: Option<Arc<dyn MediaAttachmentVerifier + Send + Sync>>,
    session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>>,
    user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>>,
}

impl MessagePersistenceDomainService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        idempotency_repo: Option<Arc<dyn MessageIdempotencyRepository + Send + Sync>>,
        hot_cache_repo: Option<Arc<dyn HotCacheRepository + Send + Sync>>,
        realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>>,
        archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
        wal_cleanup_repo: Option<Arc<dyn WalCleanupRepository + Send + Sync>>,
        ack_publisher: Option<Arc<dyn AckPublisher + Send + Sync>>,
        media_verifier: Option<Arc<dyn MediaAttachmentVerifier + Send + Sync>>,
        session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>>,
        user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>>,
    ) -> Self {
        Self {
            idempotency_repo,
            hot_cache_repo,
            realtime_repo,
            archive_repo,
            wal_cleanup_repo,
            ack_publisher,
            media_verifier,
            session_state_repo,
            user_cursor_repo,
        }
    }

    /// 准备消息（从请求中提取并准备消息）
    pub fn prepare_message(&self, request: StoreMessageRequest) -> Result<PreparedMessage> {
        let session_id = if request.session_id.is_empty() {
            request
                .message
                .as_ref()
                .map(|msg| msg.session_id.clone())
                .unwrap_or_default()
        } else {
            request.session_id.clone()
        };

        let mut message = request
            .message
            .ok_or_else(|| anyhow!("missing message payload"))?;

        if message.session_id.is_empty() {
            message.session_id = session_id.clone();
        }

        if message.id.is_empty() {
            message.id = Uuid::new_v4().to_string();
        }

        let mut timeline = extract_timeline_from_extra(&message.extra, current_millis());
        let persisted_ts = current_millis();
        timeline.persisted_ts = Some(persisted_ts);

        // 确保时间线信息嵌入到消息的 extra 中
        flare_im_core::utils::embed_timeline_in_extra(&mut message, &timeline);

        Ok(PreparedMessage {
            session_id,
            message_id: message.id.clone(),
            message,
            timeline,
            sync: request.sync,
        })
    }

    /// 验证并补全媒资附件
    #[instrument(skip(self))]
    pub async fn verify_and_enrich_media(&self, message: &mut Message) -> Result<()> {
        if let Some(verifier) = &self.media_verifier {
            if let Some(media_refs_raw) = message.extra.get("media_refs") {
                match serde_json::from_str::<Vec<String>>(media_refs_raw) {
                    Ok(media_ids) if !media_ids.is_empty() => {
                        match verifier.fetch_metadata(&media_ids).await {
                            Ok(metadata) => {
                                if let Ok(serialized) = serde_json::to_string(&metadata) {
                                    message
                                        .extra
                                        .insert("media_attachments".to_string(), serialized);
                                }
                            }
                            Err(err) => {
                                warn!(error = ?err, "Failed to resolve media attachments");
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(err) => warn!(error = ?err, "Invalid media_refs payload"),
                }
            }
        }
        Ok(())
    }

    /// 检查消息是否为新消息（幂等性检查）
    #[instrument(skip(self), fields(message_id = %prepared.message_id))]
    pub async fn check_idempotency(&self, prepared: &PreparedMessage) -> Result<bool> {
        match &self.idempotency_repo {
            Some(repo) => match repo.is_new(&prepared.message_id).await {
                Ok(value) => Ok(value),
                Err(err) => {
                    warn!(
                        error = ?err,
                        message_id = %prepared.message_id,
                        "Idempotency check failed; treating as new"
                    );
                    Ok(true)
                }
            },
            None => Ok(true),
        }
    }

    /// 持久化消息到存储
    #[instrument(skip(self), fields(message_id = %prepared.message_id))]
    pub async fn persist_message(&self, prepared: &PreparedMessage) -> Result<()> {
        // 数据库写入
        if let Some(repo) = &self.hot_cache_repo {
            repo.store_hot(&prepared.message).await?;
        }
        if let Some(repo) = &self.realtime_repo {
            repo.store_realtime(&prepared.message).await?;
        }
        if let Some(repo) = &self.archive_repo {
            repo.store_archive(&prepared.message).await?;
        }

        // Redis 更新
        if let Some(repo) = &self.session_state_repo {
            repo.apply_message(&prepared.message).await?;
        }
        if let Some(cursor_repo) = &self.user_cursor_repo {
            for receiver in &prepared.message.receiver_ids {
                cursor_repo
                    .advance_cursor(
                        &prepared.session_id,
                        receiver,
                        prepared.timeline.ingestion_ts,
                    )
                    .await?;
            }
            if !prepared.message.receiver_id.is_empty() {
                cursor_repo
                    .advance_cursor(
                        &prepared.session_id,
                        &prepared.message.receiver_id,
                        prepared.timeline.ingestion_ts,
                    )
                    .await?;
            }
            cursor_repo
                .advance_cursor(
                    &prepared.session_id,
                    &prepared.message.sender_id,
                    prepared.timeline.ingestion_ts,
                )
                .await?;
        }

        Ok(())
    }

    /// 清理 WAL 条目
    #[instrument(skip(self), fields(message_id = %message_id))]
    pub async fn cleanup_wal(&self, message_id: &str) -> Result<()> {
        if let Some(repo) = &self.wal_cleanup_repo {
            if let Err(err) = repo.remove(message_id).await {
                warn!(
                    error = ?err,
                    message_id = %message_id,
                    "Failed to cleanup WAL entry"
                );
            }
        }
        Ok(())
    }

    /// 发布 ACK 事件
    #[instrument(skip(self), fields(message_id = %result.message_id))]
    pub async fn publish_ack(&self, result: &PersistenceResult) -> Result<()> {
        if let Some(publisher) = &self.ack_publisher {
            let persisted_ts = result.timeline.persisted_ts.unwrap_or_else(current_millis);
            let event = AckEvent {
                message_id: &result.message_id,
                session_id: &result.session_id,
                status: AckStatus::from_deduplicated(result.deduplicated),
                ingestion_ts: result.timeline.ingestion_ts,
                persisted_ts,
                deduplicated: result.deduplicated,
            };
            if let Err(err) = publisher.publish(event).await {
                warn!(
                    error = ?err,
                    message_id = %result.message_id,
                    "Failed to publish persistence ACK"
                );
            }
        }
        Ok(())
    }

}

