//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;
use std::time::Instant;
use anyhow::Result;
use tracing::instrument;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_message_id, set_tenant_id};
use flare_im_core::metrics::StorageWriterMetrics;

use crate::application::commands::ProcessStoreMessageCommand;
use crate::domain::model::PersistenceResult;
use crate::domain::service::MessagePersistenceDomainService;

/// 消息持久化命令处理器（编排层）
/// 
/// 负责编排领域服务，并处理应用层关注点（如指标记录、追踪等）
pub struct MessagePersistenceCommandHandler {
    domain_service: Arc<MessagePersistenceDomainService>,
    metrics: Arc<StorageWriterMetrics>,
}

impl MessagePersistenceCommandHandler {
    pub fn new(
        domain_service: Arc<MessagePersistenceDomainService>,
        metrics: Arc<StorageWriterMetrics>,
    ) -> Self {
        Self {
            domain_service,
            metrics,
        }
    }

    /// 处理存储消息命令
    #[instrument(skip(self), fields(tenant_id, message_id))]
    pub async fn handle(&self, command: ProcessStoreMessageCommand) -> Result<PersistenceResult> {
        let start = Instant::now();
        let mut request = command.request;
        let span = tracing::Span::current();
        
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
                if !message.id.is_empty() {
                    set_message_id(&span, &message.id);
                    span.record("message_id", &message.id);
                }
            }
        }

        // 准备消息
        let mut prepared = self.domain_service.prepare_message(request)?;

        // 保存必要信息用于后续操作（因为 prepared 会被移动到 persist_message）
        let message_id = prepared.message_id.clone();
        let session_id = prepared.session_id.clone();
        let timeline = prepared.timeline.clone();

        // 验证并补全媒资附件
        self.domain_service.verify_and_enrich_media(&mut prepared.message).await?;

        // 检查幂等性
        let is_new = self.domain_service.check_idempotency(&prepared).await?;
        let deduplicated = !is_new;
        
        // 记录去重统计（应用层关注点）
        if deduplicated {
            self.metrics.messages_duplicate_total.inc();
        }

        if is_new {
            // 数据库写入
            #[cfg(feature = "tracing")]
            let db_span = create_span("storage-writer", "db_write");
            
            let db_start = Instant::now();
            self.domain_service.persist_message(prepared).await?;
            let db_duration = db_start.elapsed();
            
            // 记录数据库写入耗时（应用层关注点）
            self.metrics.db_write_duration_seconds.observe(db_duration.as_secs_f64());
            
            #[cfg(feature = "tracing")]
            db_span.end();
            
            // 记录总耗时和持久化计数（应用层关注点）
            let total_duration = start.elapsed();
            self.metrics.messages_persisted_duration_seconds.observe(total_duration.as_secs_f64());
            self.metrics.messages_persisted_total
                .with_label_values(&[&tenant_id])
                .inc();
        }

        // 清理 WAL
        self.domain_service.cleanup_wal(&message_id).await?;

        // 构建结果（使用已保存的信息）
        let result = PersistenceResult {
            session_id,
            message_id,
            timeline,
            deduplicated,
        };
        
        // 发布 ACK 事件
        self.domain_service.publish_ack(&result).await?;

        Ok(result)
    }
}

