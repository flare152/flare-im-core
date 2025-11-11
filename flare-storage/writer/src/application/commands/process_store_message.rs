use std::sync::Arc;

use anyhow::Result;
use flare_proto::storage::StoreMessageRequest;
use flare_storage_model::current_millis;
use serde_json;
use tracing::warn;

use crate::domain::events::{AckEvent, AckStatus};
use crate::domain::message_persistence::{PersistenceResult, PreparedMessage};
use crate::domain::repositories::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, WalCleanupRepository,
};

/// 包装来自编排层的 `StoreMessageRequest`，驱动写入流程。
pub struct ProcessStoreMessageCommand {
    pub request: StoreMessageRequest,
}

/// 消息持久化用例处理器，负责幂等校验、冷热存储划分、WAL 清理与 ACK 事件发布。
pub struct ProcessStoreMessageCommandHandler {
    idempotency_repo: Option<Arc<dyn MessageIdempotencyRepository + Send + Sync>>,
    hot_cache_repo: Option<Arc<dyn HotCacheRepository + Send + Sync>>,
    realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>>,
    archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
    wal_cleanup_repo: Option<Arc<dyn WalCleanupRepository + Send + Sync>>,
    ack_publisher: Option<Arc<dyn AckPublisher + Send + Sync>>,
    media_verifier: Option<Arc<dyn MediaAttachmentVerifier + Send + Sync>>,
}

impl ProcessStoreMessageCommandHandler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        idempotency_repo: Option<Arc<dyn MessageIdempotencyRepository + Send + Sync>>,
        hot_cache_repo: Option<Arc<dyn HotCacheRepository + Send + Sync>>,
        realtime_repo: Option<Arc<dyn RealtimeStoreRepository + Send + Sync>>,
        archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
        wal_cleanup_repo: Option<Arc<dyn WalCleanupRepository + Send + Sync>>,
        ack_publisher: Option<Arc<dyn AckPublisher + Send + Sync>>,
        media_verifier: Option<Arc<dyn MediaAttachmentVerifier + Send + Sync>>,
    ) -> Self {
        Self {
            idempotency_repo,
            hot_cache_repo,
            realtime_repo,
            archive_repo,
            wal_cleanup_repo,
            ack_publisher,
            media_verifier,
        }
    }

    /// 执行“媒资补全 → 幂等校验 → 多存储写入 → WAL 清理 → ACK 发布”的完整落地流程。
    pub async fn handle(&self, command: ProcessStoreMessageCommand) -> Result<PersistenceResult> {
        let mut request = command.request;

        if let Some(verifier) = &self.media_verifier {
            if let Some(message) = request.message.as_mut() {
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
        }

        let prepared = PreparedMessage::from_request(request)?;

        let is_new = match &self.idempotency_repo {
            Some(repo) => match repo.is_new(&prepared.message_id).await {
                Ok(value) => value,
                Err(err) => {
                    warn!(
                        error = ?err,
                        message_id = %prepared.message_id,
                        "Idempotency check failed; treating as new"
                    );
                    true
                }
            },
            None => true,
        };
        let deduplicated = !is_new;

        if is_new {
            if let Some(repo) = &self.hot_cache_repo {
                repo.store_hot(&prepared.stored_message).await?;
            }
            if let Some(repo) = &self.realtime_repo {
                repo.store_realtime(&prepared.stored_message).await?;
            }
            if let Some(repo) = &self.archive_repo {
                repo.store_archive(&prepared.message).await?;
            }
        }

        if let Some(repo) = &self.wal_cleanup_repo {
            if let Err(err) = repo.remove(&prepared.message_id).await {
                warn!(
                    error = ?err,
                    message_id = %prepared.message_id,
                    "Failed to cleanup WAL entry"
                );
            }
        }

        let result = PersistenceResult::new(&prepared, deduplicated);

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

        Ok(result)
    }
}
