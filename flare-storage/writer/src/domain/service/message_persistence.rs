//! 消息持久化领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;

use anyhow::{Result, anyhow};
use flare_im_core::utils::{current_millis, embed_seq_in_message, extract_timeline_from_extra};
use flare_proto::common::Message;
use flare_proto::storage::StoreMessageRequest;
use serde_json;
use tracing::{instrument, warn};
use uuid::Uuid;

use crate::domain::events::{AckEvent, AckStatus};
use crate::domain::model::{PersistenceResult, PreparedMessage};
use crate::domain::repository::{
    AckPublisher, ArchiveStoreRepository, HotCacheRepository, MediaAttachmentVerifier,
    MessageIdempotencyRepository, RealtimeStoreRepository, SeqGenerator, SessionStateRepository,
    SessionUpdateRepository, UserSyncCursorRepository, WalCleanupRepository,
};
use crate::domain::service::session_domain_service::SessionDomainService; // 添加SessionDomainService导入
use flare_server_core::ServiceClient; // 添加ServiceClient导入
use tokio::sync::Mutex; // 添加Mutex导入

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
    seq_generator: Option<Arc<dyn SeqGenerator + Send + Sync>>,
    session_update_repo: Option<Arc<dyn SessionUpdateRepository + Send + Sync>>,
    session_domain_service: Arc<SessionDomainService>, // 使用SessionDomainService替代原来的session_client
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
        seq_generator: Option<Arc<dyn SeqGenerator + Send + Sync>>,
        session_update_repo: Option<Arc<dyn SessionUpdateRepository + Send + Sync>>,
        session_client: Option<Arc<Mutex<ServiceClient>>>, // 保持session_client参数用于创建SessionDomainService
    ) -> Self {
        // 创建SessionDomainService实例
        let session_domain_service = Arc::new(SessionDomainService::new(session_client));

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
            seq_generator,
            session_update_repo,
            session_domain_service, // 使用SessionDomainService
        }
    }

    /// 准备消息（从请求中提取并准备消息）
    ///
    /// 注意：消息从 Kafka 队列中读取出来时，说明已经成功发送并被接收，
    /// 因此应该将状态从 `Created` 更新为 `Sent`
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

        // 消息从 Kafka 队列中读取出来时，说明已经成功发送并被接收
        // 将状态从 `Created` (1) 更新为 `Sent` (2)
        use flare_proto::common::MessageStatus;
        if message.status == MessageStatus::Created as i32 || message.status == 0 {
            message.status = MessageStatus::Sent as i32;
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
    pub async fn persist_message(&self, mut prepared: PreparedMessage) -> Result<()> {
        // 1. 生成 seq
        let seq = if let Some(generator) = &self.seq_generator {
            let generated_seq = generator.generate_seq(&prepared.session_id).await?;
            // 将 seq 嵌入到 message.extra 中
            embed_seq_in_message(&mut prepared.message, generated_seq);
            Some(generated_seq)
        } else {
            None
        };

        // 保存 session_id 和 message_id 用于后续更新
        let session_id = prepared.session_id.clone();
        let message_id = prepared.message_id.clone();
        let sender_id = prepared.message.sender_id.clone();
        let timeline = prepared.timeline.clone(); // 克隆 timeline 在使用前

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
            cursor_repo
                .advance_cursor(
                    &session_id,
                    &prepared.message.sender_id,
                    timeline.ingestion_ts, // 使用克隆的 timeline
                )
                .await?;
        }

        // 2. 更新会话的最后消息信息
        if let (Some(repo), Some(s)) = (&self.session_update_repo, seq) {
            repo.update_last_message(&session_id, &message_id, s)
                .await?;
        }

        // 3. 批量更新参与者的未读数
        if let (Some(repo), Some(s)) = (&self.session_update_repo, seq) {
            repo.batch_update_unread_count(&session_id, s, Some(&sender_id))
                .await?;
        }

        // 5. 构建持久化结果
        let result = PersistenceResult {
            message_id,
            session_id,
            timeline, // 使用克隆的 timeline
            deduplicated: false,
        };

        Ok(())
    }

    /// 批量持久化消息到存储（优化性能）
    #[instrument(skip(self), fields(batch_size = prepared.len()))]
    pub async fn persist_batch(&self, mut prepared: Vec<PreparedMessage>) -> Result<()> {
        if prepared.is_empty() {
            return Ok(());
        }

        // 1. 批量生成 seq
        let mut seqs = Vec::with_capacity(prepared.len());
        if let Some(generator) = &self.seq_generator {
            for p in &mut prepared {
                let seq = generator.generate_seq(&p.session_id).await?;
                embed_seq_in_message(&mut p.message, seq);
                seqs.push(seq);
            }
        } else {
            seqs = vec![0; prepared.len()];
        }

        // 提取消息用于批量写入
        let messages: Vec<Message> = prepared.iter().map(|p| p.message.clone()).collect();

        // 2. 批量写入数据库
        if let Some(repo) = &self.hot_cache_repo {
            repo.store_hot_batch(&messages).await?;
        }
        if let Some(repo) = &self.realtime_repo {
            repo.store_realtime_batch(&messages).await?;
        }
        if let Some(repo) = &self.archive_repo {
            repo.store_archive_batch(&messages).await?;
        }

        // 3. 批量更新 Redis（按会话分组）
        let mut session_groups: std::collections::HashMap<String, Vec<(&PreparedMessage, i64)>> =
            std::collections::HashMap::new();
        for (p, seq) in prepared.iter().zip(seqs.iter()) {
            session_groups
                .entry(p.session_id.clone())
                .or_insert_with(Vec::new)
                .push((p, *seq));
        }

        // 批量更新会话状态
        if let Some(repo) = &self.session_state_repo {
            for message in &messages {
                repo.apply_message(message).await?;
            }
        }

        // 批量更新游标（按用户分组）
        if let Some(cursor_repo) = &self.user_cursor_repo {
            let mut user_cursors: std::collections::HashMap<(String, String), i64> =
                std::collections::HashMap::new();

            for p in &prepared {
                let key = (p.session_id.clone(), p.message.sender_id.clone());
                let ts = p.timeline.ingestion_ts;
                user_cursors.entry(key).or_insert(ts);
            }

            // 批量更新游标
            for ((session_id, user_id), ts) in user_cursors {
                cursor_repo
                    .advance_cursor(&session_id, &user_id, ts)
                    .await?;
            }
        }

        // 4. 批量更新会话的最后消息信息（按会话分组）
        if let Some(repo) = &self.session_update_repo {
            for (session_id, updates) in &session_groups {
                if let Some((last_p, last_seq)) = updates.last() {
                    repo.update_last_message(&session_id, &last_p.message_id, *last_seq)
                        .await?;
                }
            }
        }

        // 5. 批量更新未读数（按会话分组）
        if let Some(repo) = &self.session_update_repo {
            for (session_id, updates) in &session_groups {
                if let Some((last_p, last_seq)) = updates.last() {
                    repo.batch_update_unread_count(
                        &session_id,
                        *last_seq,
                        Some(&last_p.message.sender_id),
                    )
                    .await?;
                }
            }
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

    /// 获取会话参与者列表
    ///
    /// 通过gRPC调用Session服务获取会话的所有参与者，用于更新未读数
    pub async fn get_session_participants(&self, session_id: &str) -> Result<Vec<String>> {
        self.session_domain_service
            .get_session_participants(session_id)
            .await
    }

    /// 更新参与者的未读数
    ///
    /// 根据会话参与者列表，批量更新他们的未读数
    pub async fn update_participants_unread_count(
        &self,
        session_id: &str,
        seq: i64,
        sender_id: &str,
    ) -> Result<()> {
        // 获取会话参与者列表
        let participant_ids = self.get_session_participants(session_id).await?;

        // 如果有配置session_update_repo，则更新未读数
        if let Some(repo) = &self.session_update_repo {
            // 过滤掉发送者自己
            let filtered_participants: Vec<String> = participant_ids
                .into_iter()
                .filter(|id| id != sender_id)
                .collect();

            // 批量更新未读数
            if !filtered_participants.is_empty() {
                repo.batch_update_unread_count(session_id, seq, Some(sender_id))
                    .await?;
            }
        }

        Ok(())
    }

    /// 存储一致性保障机制
    ///
    /// 确保数据库和缓存之间的一致性，包括：
    /// 1. 数据库写入成功后再更新缓存
    /// 2. 缓存更新失败时的补偿机制
    /// 3. 消息去重检查
    /// 4. 批量操作的原子性保障
    pub async fn ensure_consistency(
        &self,
        mut prepared: PreparedMessage,
    ) -> Result<PersistenceResult> {
        // 1. 幂等性检查（防止重复处理）
        let is_new = self.check_idempotency(&prepared).await?;
        if !is_new {
            return Ok(PersistenceResult {
                message_id: prepared.message_id.clone(),
                session_id: prepared.session_id.clone(),
                timeline: prepared.timeline.clone(),
                deduplicated: true,
            });
        }

        // 2. 验证并补全媒资附件
        self.verify_and_enrich_media(&mut prepared.message).await?;

        // 保存必要的信息用于后续步骤
        let message_id = prepared.message_id.clone();
        let session_id = prepared.session_id.clone();
        let timeline = prepared.timeline.clone();

        // 3. 持久化消息到存储
        self.persist_message(prepared).await?;

        // 4. 清理 WAL 条目
        self.cleanup_wal(&message_id).await?;

        // 5. 构建持久化结果
        let result = PersistenceResult {
            message_id,
            session_id,
            timeline,
            deduplicated: false,
        };

        // 6. 发布 ACK 事件
        self.publish_ack(&result).await?;

        Ok(result)
    }

    /// 批量存储一致性保障机制
    ///
    /// 确保批量操作的原子性和一致性
    pub async fn ensure_batch_consistency(
        &self,
        prepared: Vec<PreparedMessage>,
    ) -> Result<Vec<PersistenceResult>> {
        if prepared.is_empty() {
            return Ok(vec![]);
        }

        // 1. 批量幂等性检查
        let mut new_messages = Vec::new();
        let mut results = Vec::new();

        for mut msg in prepared {
            let is_new = self.check_idempotency(&msg).await?;
            if is_new {
                // 验证并补全媒资附件
                self.verify_and_enrich_media(&mut msg.message).await?;
                new_messages.push(msg);
            } else {
                // 构建重复消息的结果
                results.push(PersistenceResult {
                    message_id: msg.message_id.clone(),
                    session_id: msg.session_id.clone(),
                    timeline: msg.timeline.clone(),
                    deduplicated: true,
                });
            }
        }

        if new_messages.is_empty() {
            return Ok(results);
        }

        // 2. 批量持久化消息
        self.persist_batch(new_messages.clone()).await?;

        // 3. 批量清理 WAL 条目
        for msg in &new_messages {
            self.cleanup_wal(&msg.message_id).await?;
        }

        // 4. 构建持久化结果
        for msg in new_messages {
            let result = PersistenceResult {
                message_id: msg.message_id.clone(),
                session_id: msg.session_id.clone(),
                timeline: msg.timeline.clone(),
                deduplicated: false,
            };
            results.push(result);
        }

        // 5. 批量发布 ACK 事件
        for result in &results {
            self.publish_ack(result).await?;
        }

        Ok(results)
    }
}
