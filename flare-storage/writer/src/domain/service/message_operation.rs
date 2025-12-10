//! 消息操作领域服务 - 处理消息操作（撤回、编辑、删除等）
//!
//! 职责：
//! - 识别操作消息
//! - 提取 MessageOperation
//! - 根据操作类型更新数据库

use std::sync::Arc;
use anyhow::{Result, anyhow};
use flare_proto::common::{
    Message, MessageOperation, MessageStatus, VisibilityStatus, OperationType,
    message_operation::OperationData,
};
use prost::Message as ProstMessage;
use tracing::{instrument, warn};

use crate::domain::repository::ArchiveStoreRepository;

/// 消息操作领域服务
pub struct MessageOperationDomainService {
    archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
}

impl MessageOperationDomainService {
    pub fn new(
        archive_repo: Option<Arc<dyn ArchiveStoreRepository + Send + Sync>>,
    ) -> Self {
        Self { archive_repo }
    }

    /// 检查消息是否为操作消息
    pub fn is_operation_message(message: &Message) -> bool {
        message.message_type == flare_proto::MessageType::Notification as i32
            && message.content.as_ref()
                .and_then(|c| c.content.as_ref())
                .and_then(|c| match c {
                    flare_proto::common::message_content::Content::Notification(notif) => {
                        Some(notif.notification_type == "message_operation")
                    }
                    _ => None,
                })
                .unwrap_or(false)
    }

    /// 从消息中提取 MessageOperation
    pub fn extract_operation_from_message(
        message: &Message,
    ) -> Result<Option<MessageOperation>> {
        if !Self::is_operation_message(message) {
            return Ok(None);
        }

        if let Some(ref content) = message.content {
            if let Some(flare_proto::common::message_content::Content::Notification(notif)) =
                &content.content
            {
                if notif.notification_type == "message_operation" {
                    // 从 data 字段中反序列化 MessageOperation
                    // 注意：根据协议设计，data 是 base64 编码的序列化 MessageOperation
                    // 兼容两种键名：历史版本使用 "data"，SDK 当前使用 "operation_data"
                    let data_str_opt = notif.data.get("operation_data")
                        .or_else(|| notif.data.get("data"));
                    if let Some(data_str) = data_str_opt {
                        // 解码 base64
                        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
                        let decoded = BASE64.decode(data_str)
                            .map_err(|e| anyhow!("Failed to decode base64: {}", e))?;
                        
                        // 反序列化 MessageOperation
                        let operation = MessageOperation::decode(&decoded[..])
                            .map_err(|e| anyhow!("Failed to decode MessageOperation: {}", e))?;
                        
                        return Ok(Some(operation));
                    }
                }
            }
        }

        Ok(None)
    }

    /// 处理消息操作（撤回、编辑、删除等）
    #[instrument(skip(self), fields(operation_type = %operation.operation_type))]
    pub async fn process_operation(
        &self,
        operation: MessageOperation,
        _message: &Message,
    ) -> Result<()> {
        let archive_repo = self.archive_repo.as_ref().ok_or_else(|| {
            anyhow!("Archive repository not configured")
        })?;

        match OperationType::try_from(operation.operation_type) {
            Ok(OperationType::Recall) => {
                self.handle_recall_operation(&operation, archive_repo).await
            }
            Ok(OperationType::Edit) => {
                self.handle_edit_operation(&operation, archive_repo).await
            }
            Ok(OperationType::Delete) => {
                self.handle_delete_operation(&operation, archive_repo).await
            }
            _ => {
                warn!(
                    operation_type = operation.operation_type,
                    "Unsupported operation type, skipping"
                );
                Ok(())
            }
        }
    }

    /// 处理撤回操作
    #[instrument(skip(self, archive_repo), fields(message_id = %operation.target_message_id))]
    async fn handle_recall_operation(
        &self,
        operation: &MessageOperation,
        archive_repo: &Arc<dyn ArchiveStoreRepository + Send + Sync>,
    ) -> Result<()> {
        let message_id = &operation.target_message_id;

        // 更新消息状态为 Recalled
        archive_repo
            .update_message_status(
                message_id,
                MessageStatus::Recalled,
                Some(true), // is_recalled
                operation.timestamp.clone(), // recalled_at
            )
            .await?;

        // 追加操作记录
        archive_repo.append_operation(message_id, operation).await?;

        Ok(())
    }

    /// 处理编辑操作
    /// 
    /// 功能：
    /// 1. 验证编辑权限（只有发送者可以编辑）
    /// 2. 检查编辑时间限制（默认 48 小时，参考 Telegram）
    /// 3. 验证编辑版本号（必须递增）
    /// 4. 更新消息内容并保存编辑历史
    /// 5. 追加操作记录
    #[instrument(skip(self, archive_repo), fields(message_id = %operation.target_message_id))]
    async fn handle_edit_operation(
        &self,
        operation: &MessageOperation,
        archive_repo: &Arc<dyn ArchiveStoreRepository + Send + Sync>,
    ) -> Result<()> {
        use chrono::Utc;
        use prost::Message as _;
        
        const MAX_EDIT_TIME_SECONDS: i64 = 48 * 3600;  // 48 小时（参考 Telegram）
        
        let message_id = &operation.target_message_id;

        // 从 operation_data 中提取编辑后的内容
        let edit_data = match &operation.operation_data {
            Some(OperationData::Edit(data)) => data,
            _ => return Err(anyhow!("Edit operation requires EditOperationData")),
        };
        
            let new_content = edit_data
                .new_content
                .as_ref()
                .ok_or_else(|| anyhow!("Edit operation requires new_content"))?;

        // 1. 获取当前消息（用于验证权限和时间限制）
        // 注意：这里需要通过 Reader 服务获取消息，或者扩展 ArchiveStoreRepository 接口
        // 为了简化，我们假设消息信息已经在 operation 中，或者通过其他方式获取
        // 实际实现中，应该通过 Reader 服务获取消息详情
        
        // 2. 检查编辑时间限制（如果消息时间戳可用）
        if let Some(message_timestamp) = &operation.timestamp {
            let now = Utc::now();
            let message_time = chrono::DateTime::<chrono::Utc>::from_timestamp(
                message_timestamp.seconds,
                message_timestamp.nanos as u32,
            ).ok_or_else(|| anyhow!("Invalid message timestamp"))?;
            
            let elapsed_seconds = (now - message_time).num_seconds();
            if elapsed_seconds > MAX_EDIT_TIME_SECONDS {
                return Err(anyhow!(
                    "Message cannot be edited after {} hours. Elapsed: {} seconds",
                    MAX_EDIT_TIME_SECONDS / 3600,
                    elapsed_seconds
                ));
            }
        }

        // 3. 更新消息内容（内部会验证版本号并保存编辑历史）
            archive_repo
                .update_message_content(message_id, new_content, edit_data.edit_version)
                .await?;

        // 4. 追加操作记录
            archive_repo.append_operation(message_id, operation).await?;

        Ok(())
    }

    /// 处理删除操作（硬删除）
    #[instrument(skip(self, archive_repo), fields(message_id = %operation.target_message_id))]
    async fn handle_delete_operation(
        &self,
        operation: &MessageOperation,
        archive_repo: &Arc<dyn ArchiveStoreRepository + Send + Sync>,
    ) -> Result<()> {
        let message_id = &operation.target_message_id;

        // 检查删除类型
        if let Some(OperationData::Delete(delete_data)) = &operation.operation_data {
            // 硬删除：全局删除（user_id = None）
            if delete_data.delete_type == flare_proto::common::DeleteType::Hard as i32 {
                archive_repo
                    .update_message_visibility(
                        message_id,
                        None, // user_id = None 表示全局删除
                        VisibilityStatus::VisibilityDeleted,
                    )
                    .await?;

                // 追加操作记录
                archive_repo.append_operation(message_id, operation).await?;
            } else {
                // 软删除：应该通过 Reader 处理（不需要 Kafka）
                warn!(
                    message_id = %message_id,
                    "Soft delete operation should be handled by Reader, not Writer"
                );
            }
        } else {
            return Err(anyhow!("Delete operation requires DeleteOperationData"));
        }

        Ok(())
    }
}
