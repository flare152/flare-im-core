//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use anyhow::Result;
use flare_im_core::metrics::StorageWriterMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_tenant_id};
use std::sync::Arc;
use std::time::Instant;
use tracing::instrument;

use crate::application::commands::{ProcessMessageOperationCommand, ProcessStoreMessageCommand};
use crate::domain::model::{PersistenceResult, PreparedMessage};
use crate::domain::service::{MessageOperationDomainService, MessagePersistenceDomainService};

/// 消息持久化命令处理器（编排层）
///
/// 负责编排领域服务，并处理应用层关注点（如指标记录、追踪等）
pub struct MessagePersistenceCommandHandler {
    domain_service: Arc<MessagePersistenceDomainService>,
    operation_service: Arc<MessageOperationDomainService>,
    metrics: Arc<StorageWriterMetrics>,
}

impl MessagePersistenceCommandHandler {
    pub fn new(
        domain_service: Arc<MessagePersistenceDomainService>,
        operation_service: Arc<MessageOperationDomainService>,
        metrics: Arc<StorageWriterMetrics>,
    ) -> Self {
        Self {
            domain_service,
            operation_service,
            metrics,
        }
    }

    /// 处理消息操作命令（撤回、编辑、删除等）
    #[instrument(skip(self), fields(operation_type = %command.operation.operation_type))]
    pub async fn handle_operation_message(
        &self,
        command: ProcessMessageOperationCommand,
    ) -> Result<PersistenceResult> {
        let start = Instant::now();

        // 处理操作
        self.operation_service
            .process_operation(command.operation.clone(), &command.message)
            .await?;

        // 构建结果（操作消息不创建新消息，返回操作的目标消息ID）
        let result = PersistenceResult {
            session_id: command.message.session_id.clone(),
            message_id: command.operation.target_message_id.clone(),
            timeline: Default::default(),
            deduplicated: false,
        };

        // 记录指标
        let duration = start.elapsed();
        self.metrics
            .messages_persisted_duration_seconds
            .observe(duration.as_secs_f64());
        self.metrics
            .messages_persisted_total
            .with_label_values(&["operation"])
            .inc();

        tracing::info!(
            message_id = %result.message_id,
            operation_type = %command.operation.operation_type,
            duration_ms = duration.as_millis(),
            "Message operation processed successfully"
        );

        Ok(result)
    }

    /// 处理存储消息命令
    #[instrument(skip(self), fields(tenant_id, message_id))]
    pub async fn handle(&self, command: ProcessStoreMessageCommand) -> Result<PersistenceResult> {
        let start = Instant::now();
        let request = command.request;

        // 在移动 request 之前，先保存需要的信息用于错误日志
        let message_id_for_error = request.message.as_ref().map(|m| m.id.clone());

        // 提取租户ID用于指标
        let tenant_id = request
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("unknown")
            .to_string();

        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            let span = tracing::Span::current();
            set_tenant_id(&span, &tenant_id);
            if let Some(message) = &request.message {
                if !message.id.is_empty() {
                    set_message_id(&span, &message.id);
                    span.record("message_id", &message.id);
                }
            }
        }

        // 准备消息
        let mut prepared = match self.domain_service.prepare_message(request) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    message_id = ?message_id_for_error,
                    "Failed to prepare message"
                );
                return Err(e);
            }
        };

        // 保存必要信息用于后续操作（因为 prepared 会被移动到 persist_message）
        let message_id = prepared.message_id.clone();
        let session_id = prepared.session_id.clone();
        let timeline = prepared.timeline.clone();

        // 验证并补全媒资附件
        if let Err(e) = self
            .domain_service
            .verify_and_enrich_media(&mut prepared.message)
            .await
        {
            tracing::error!(error = %e, message_id = %message_id, "Failed to verify and enrich media");
            return Err(e);
        }

        // 检查幂等性
        let is_new = match self.domain_service.check_idempotency(&prepared).await {
            Ok(new) => new,
            Err(e) => {
                tracing::error!(error = %e, message_id = %message_id, "Failed to check idempotency");
                return Err(e);
            }
        };
        let deduplicated = !is_new;

        // 记录去重统计（应用层关注点）
        if deduplicated {
            self.metrics.messages_duplicate_total.inc();
            tracing::debug!(message_id = %message_id, "Message is duplicate, skipping persistence");
        }

        if is_new {
            // 数据库写入
            #[cfg(feature = "tracing")]
            let db_span = create_span("storage-writer", "db_write");

            let db_start = Instant::now();
            match self.domain_service.persist_message(prepared).await {
                Ok(_) => {
                    let db_duration = db_start.elapsed();

                    // 记录数据库写入耗时（应用层关注点）
                    self.metrics
                        .db_write_duration_seconds
                        .observe(db_duration.as_secs_f64());

                    #[cfg(feature = "tracing")]
                    db_span.end();

                    // 记录总耗时和持久化计数（应用层关注点）
                    let total_duration = start.elapsed();
                    self.metrics
                        .messages_persisted_duration_seconds
                        .observe(total_duration.as_secs_f64());
                    self.metrics
                        .messages_persisted_total
                        .with_label_values(&[&tenant_id])
                        .inc();

                    tracing::info!(
                        message_id = %message_id,
                        session_id = %session_id,
                        duration_ms = total_duration.as_millis(),
                        "Message persisted successfully"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        message_id = %message_id,
                        session_id = %session_id,
                        "Failed to persist message to database"
                    );
                    return Err(e);
                }
            }
        }

        // 清理 WAL（即使失败也不影响消息持久化，只记录警告）
        if let Err(e) = self.domain_service.cleanup_wal(&message_id).await {
            tracing::warn!(error = %e, message_id = %message_id, "Failed to cleanup WAL, but message is already persisted");
        }

        // 构建结果（使用已保存的信息）
        let result = PersistenceResult {
            session_id,
            message_id,
            timeline,
            deduplicated,
        };

        // 发布 ACK 事件（即使失败也不影响消息持久化，只记录警告）
        if let Err(e) = self.domain_service.publish_ack(&result).await {
            tracing::warn!(error = %e, message_id = %result.message_id, "Failed to publish ACK, but message is already persisted");
        }

        Ok(result)
    }

    /// 批量处理存储消息命令（优化性能）
    #[instrument(skip(self), fields(batch_size = commands.len()))]
    pub async fn handle_batch(
        &self,
        commands: Vec<ProcessStoreMessageCommand>,
    ) -> Result<Vec<PersistenceResult>> {
        let start = Instant::now();

        // 1. 批量准备消息
        let mut prepared_messages = Vec::with_capacity(commands.len());
        for command in commands {
            match self.domain_service.prepare_message(command.request) {
                Ok(mut prepared) => {
                    // 验证并补全媒资附件
                    if let Err(e) = self
                        .domain_service
                        .verify_and_enrich_media(&mut prepared.message)
                        .await
                    {
                        tracing::warn!(error = %e, message_id = %prepared.message_id, "Failed to verify media, continuing");
                    }
                    prepared_messages.push(prepared);
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to prepare message in batch");
                    // 继续处理其他消息
                }
            }
        }

        if prepared_messages.is_empty() {
            return Ok(Vec::new());
        }

        // 2. 批量检查幂等性
        let mut new_messages: Vec<PreparedMessage> = Vec::new();
        let mut deduplicated_count = 0;

        for prepared in &prepared_messages {
            match self.domain_service.check_idempotency(prepared).await {
                Ok(true) => {
                    new_messages.push(PreparedMessage::clone(prepared));
                }
                Ok(false) => {
                    deduplicated_count += 1;
                    self.metrics.messages_duplicate_total.inc();
                }
                Err(e) => {
                    tracing::warn!(error = %e, message_id = %prepared.message_id, "Idempotency check failed, treating as new");
                    new_messages.push(PreparedMessage::clone(prepared));
                }
            }
        }

        // 3. 批量持久化新消息
        if !new_messages.is_empty() {
            let db_start = Instant::now();
            match self
                .domain_service
                .persist_batch(new_messages.clone())
                .await
            {
                Ok(_) => {
                    let db_duration = db_start.elapsed();
                    self.metrics
                        .db_write_duration_seconds
                        .observe(db_duration.as_secs_f64());

                    let total_duration = start.elapsed();
                    self.metrics
                        .messages_persisted_duration_seconds
                        .observe(total_duration.as_secs_f64());
                    self.metrics
                        .messages_persisted_total
                        .with_label_values(&["batch"])
                        .inc_by(new_messages.len() as u64);

                    tracing::info!(
                        batch_size = new_messages.len(),
                        duration_ms = total_duration.as_millis(),
                        "Batch messages persisted successfully"
                    );
                }
                Err(e) => {
                    tracing::error!(error = %e, "Failed to persist batch messages");
                    return Err(e);
                }
            }
        }

        // 4. 批量清理 WAL 和发布 ACK
        let mut results = Vec::new();
        for prepared in &prepared_messages {
            let deduplicated = deduplicated_count > 0
                && !new_messages
                    .iter()
                    .any(|m| m.message_id == prepared.message_id);

            // 清理 WAL
            if let Err(e) = self.domain_service.cleanup_wal(&prepared.message_id).await {
                tracing::warn!(error = %e, message_id = %prepared.message_id, "Failed to cleanup WAL");
            }

            // 构建结果
            let result = PersistenceResult {
                session_id: prepared.session_id.clone(),
                message_id: prepared.message_id.clone(),
                timeline: prepared.timeline.clone(),
                deduplicated,
            };

            // 发布 ACK
            if let Err(e) = self.domain_service.publish_ack(&result).await {
                tracing::warn!(error = %e, message_id = %result.message_id, "Failed to publish ACK");
            }

            results.push(result);
        }

        Ok(results)
    }
}
