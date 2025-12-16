//! 命令处理器（编排层）- 轻量级，只负责编排领域服务和记录指标

use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use flare_im_core::metrics::MessageOrchestratorMetrics;
use tracing::instrument;

use crate::application::commands::{BatchStoreMessageCommand, StoreMessageCommand};
use crate::domain::service::MessageDomainService;

/// 消息命令处理器（编排层）
pub struct MessageCommandHandler {
    domain_service: Arc<MessageDomainService>,
    metrics: Arc<MessageOrchestratorMetrics>,
}

impl MessageCommandHandler {
    pub fn new(
        domain_service: Arc<MessageDomainService>,
        metrics: Arc<MessageOrchestratorMetrics>,
    ) -> Self {
        Self {
            domain_service,
            metrics,
        }
    }

    /// 处理存储消息命令
    #[instrument(skip(self))]
    pub async fn handle_store_message(&self, command: StoreMessageCommand) -> Result<String> {
        let start = Instant::now();

        // 提取租户ID和消息类型用于指标标签（在移动之前）
        let tenant_id = command
            .request
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("unknown")
            .to_string();

        let message_type = command
            .request
            .message
            .as_ref()
            .map(|m| match m.message_type {
                0 => "normal",
                _ => "notification",
            })
            .unwrap_or("normal")
            .to_string();

        let result = self
            .domain_service
            .orchestrate_message_storage(command.request, true)
            .await;

        // 记录指标
        let duration = start.elapsed();
        self.metrics
            .messages_sent_duration_seconds
            .observe(duration.as_secs_f64());

        if result.is_ok() {
            self.metrics
                .messages_sent_total
                .with_label_values(&[message_type, tenant_id])
                .inc();
        }

        result
    }

    /// 处理存储消息命令（跳过 PreSend Hook）
    #[instrument(skip(self))]
    pub async fn handle_store_message_without_pre_hook(
        &self,
        command: StoreMessageCommand,
    ) -> Result<String> {
        let start = Instant::now();

        // 提取租户ID和消息类型用于指标标签（在移动之前）
        let tenant_id = command
            .request
            .tenant
            .as_ref()
            .map(|t| t.tenant_id.as_str())
            .unwrap_or("unknown")
            .to_string();

        let message_type = command
            .request
            .message
            .as_ref()
            .map(|m| match m.message_type {
                0 => "normal",
                _ => "notification",
            })
            .unwrap_or("normal")
            .to_string();

        let result = self
            .domain_service
            .orchestrate_message_storage(command.request, false)
            .await;

        // 记录指标
        let duration = start.elapsed();
        self.metrics
            .messages_sent_duration_seconds
            .observe(duration.as_secs_f64());

        if result.is_ok() {
            self.metrics
                .messages_sent_total
                .with_label_values(&[&message_type, &tenant_id])
                .inc();
        }

        result
    }

    /// 处理批量存储消息命令
    #[instrument(skip(self), fields(batch_size = command.requests.len()))]
    pub async fn handle_batch_store_message(
        &self,
        command: BatchStoreMessageCommand,
    ) -> Result<Vec<String>> {
        let mut message_ids = Vec::new();
        for request in command.requests {
            match self
                .domain_service
                .orchestrate_message_storage(request, true)
                .await
            {
                Ok(message_id) => message_ids.push(message_id),
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to store message in batch");
                    // 继续处理其他消息
                }
            }
        }
        Ok(message_ids)
    }
}
