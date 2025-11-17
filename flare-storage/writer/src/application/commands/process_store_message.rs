use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flare_im_core::metrics::StorageWriterMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_tenant_id, set_error};
use flare_im_core::utils::current_millis;
use flare_proto::storage::StoreMessageRequest;
use serde_json;
use tracing::{warn, instrument, Span};

use crate::domain::events::{AckEvent, AckStatus};
use crate::domain::message_persistence::{PersistenceResult, PreparedMessage};
use crate::domain::repositories::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, SessionStateRepository,
    UserSyncCursorRepository, WalCleanupRepository,
};

/// 包装来自编排层的 `StoreMessageRequest`，驱动写入流程。
#[derive(Debug)]
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
    session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>>,
    user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>>,
    metrics: Arc<StorageWriterMetrics>,
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
        metrics: Arc<StorageWriterMetrics>,
    ) -> Self {
        Self {
            idempotency_repo,
            hot_cache_repo,
            realtime_repo,
            archive_repo,
            wal_cleanup_repo,
            ack_publisher,
            media_verifier,
            session_state_repo: None,
            user_cursor_repo: None,
            metrics,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_session_sync(
        mut self,
        session_state_repo: Option<Arc<dyn SessionStateRepository + Send + Sync>>,
        user_cursor_repo: Option<Arc<dyn UserSyncCursorRepository + Send + Sync>>,
    ) -> Self {
        self.session_state_repo = session_state_repo;
        self.user_cursor_repo = user_cursor_repo;
        self
    }

    /// 执行"媒资补全 → 幂等校验 → 多存储写入 → WAL 清理 → ACK 发布"的完整落地流程。
    #[instrument(skip(self), fields(tenant_id, message_id))]
    pub async fn handle(&self, command: ProcessStoreMessageCommand) -> Result<PersistenceResult> {
        let start = Instant::now();
        let mut request = command.request;
        let span = Span::current();
        
        // 提取租户ID用于指标
        let tenant_id = request.tenant.as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("unknown")
            .to_string();
        
        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            set_tenant_id(&span, &tenant_id);
            if let Some(message) = &request.message {
                if !message.message_id.is_empty() {
                    set_message_id(&span, &message.message_id);
                    span.record("message_id", &message.message_id);
                }
            }
        }

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
        
        // 记录去重统计
        if deduplicated {
            self.metrics.messages_duplicate_total.inc();
        }

        if is_new {
            // 数据库写入
            #[cfg(feature = "tracing")]
            let db_span = create_span("storage-writer", "db_write");
            
            let db_start = Instant::now();
            if let Some(repo) = &self.hot_cache_repo {
                repo.store_hot(&prepared.message).await?;
            }
            if let Some(repo) = &self.realtime_repo {
                repo.store_realtime(&prepared.message).await?;
            }
            if let Some(repo) = &self.archive_repo {
                repo.store_archive(&prepared.message).await?;
            }
            let db_duration = db_start.elapsed();
            self.metrics.db_write_duration_seconds.observe(db_duration.as_secs_f64());
            
            #[cfg(feature = "tracing")]
            db_span.end();
            
            // Redis 更新
            #[cfg(feature = "tracing")]
            let redis_span = create_span("storage-writer", "redis_update");
            
            let redis_start = Instant::now();
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
            let redis_duration = redis_start.elapsed();
            self.metrics.redis_update_duration_seconds.observe(redis_duration.as_secs_f64());
            
            #[cfg(feature = "tracing")]
            redis_span.end();
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
        
        // 记录消息持久化总数（仅新消息）
        if is_new {
            self.metrics.messages_persisted_total
                .with_label_values(&[&tenant_id])
                .inc();
        }
        
        // 记录总耗时（仅新消息）
        if is_new {
            let total_duration = start.elapsed();
            self.metrics.messages_persisted_duration_seconds.observe(total_duration.as_secs_f64());
        }

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
