//! 消息操作服务（Message Operation Service）
//!
//! 负责处理消息操作命令，执行FSM状态迁移，发布领域事件

use anyhow::{Context, Result};
use chrono::Utc;
use std::sync::Arc;
use tracing::instrument;

use crate::application::commands::{
    AddReactionCommand, BatchMarkMessageReadCommand, DeleteMessageCommand, DeleteType, EditMessageCommand,
    MarkAllConversationsReadCommand, MarkConversationReadCommand,
    MarkMessageCommand, PinMessageCommand, ReadMessageCommand,
    RecallMessageCommand, RemoveReactionCommand, UnmarkMessageCommand,
    UnpinMessageCommand,
};
use crate::domain::event::{
    MessageDeletedEvent, MessageEditedEvent, MessageFavoritedEvent, MessagePinnedEvent,
    MessageReadEvent, MessageRecalledEvent, MessageReactionAddedEvent,
    MessageReactionRemovedEvent, MessageUnfavoritedEvent, MessageUnpinnedEvent,
    MessageOperationEvent,
};
use crate::domain::model::{Message, MessageFsmState};
use crate::domain::repository::{MessageEventPublisher, WalRepository};
use crate::domain::service::message_operation_builder::MessageOperationBuilder;

/// 消息仓储接口（用于查询和保存消息）
#[async_trait::async_trait]
pub trait MessageRepository: Send + Sync {
    /// 根据消息ID查询消息
    async fn find_by_id(&self, message_id: &str) -> Result<Option<Message>>;

    /// 保存消息
    async fn save(&self, message: &Message) -> Result<()>;
}

/// 事件发布器接口
#[async_trait::async_trait]
pub trait EventPublisher: Send + Sync {
    /// 发布消息撤回事件
    async fn publish_recalled(&self, event: &MessageRecalledEvent) -> Result<()>;

    /// 发布消息编辑事件
    async fn publish_edited(&self, event: &MessageEditedEvent) -> Result<()>;

    /// 发布消息删除事件
    async fn publish_deleted(&self, event: &MessageDeletedEvent) -> Result<()>;

    /// 发布消息已读事件
    async fn publish_read(&self, event: &MessageReadEvent) -> Result<()>;

    /// 发布消息反应添加事件
    async fn publish_reaction_added(&self, event: &MessageReactionAddedEvent) -> Result<()>;

    /// 发布消息反应移除事件
    async fn publish_reaction_removed(&self, event: &MessageReactionRemovedEvent) -> Result<()>;

    /// 发布消息置顶事件
    async fn publish_pinned(&self, event: &MessagePinnedEvent) -> Result<()>;

    /// 发布消息取消置顶事件
    async fn publish_unpinned(&self, event: &MessageUnpinnedEvent) -> Result<()>;

    /// 发布消息收藏事件
    async fn publish_favorited(&self, event: &MessageFavoritedEvent) -> Result<()>;

    /// 发布消息取消收藏事件
    async fn publish_unfavorited(&self, event: &MessageUnfavoritedEvent) -> Result<()>;
}

/// 消息操作服务
pub struct MessageOperationService {
    message_repo: Arc<dyn MessageRepository>,
    event_publisher: Arc<dyn EventPublisher>,
    kafka_publisher: Arc<dyn MessageEventPublisher>,
    wal_repository: Option<Arc<crate::domain::repository::WalRepositoryItem>>,
}

impl MessageOperationService {
    pub fn new(
        message_repo: Arc<dyn MessageRepository>,
        event_publisher: Arc<dyn EventPublisher>,
        kafka_publisher: Arc<dyn MessageEventPublisher>,
        wal_repository: Option<Arc<crate::domain::repository::WalRepositoryItem>>,
    ) -> Self {
        Self {
            message_repo,
            event_publisher,
            kafka_publisher,
            wal_repository,
        }
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, operator_id = %cmd.base.operator_id))]
    pub async fn handle_recall(&self, cmd: RecallMessageCommand) -> Result<()> {
        // 验证消息存在（用于快速失败）
        let _message = self
            .message_repo
            .find_by_id(&cmd.base.message_id)
            .await?
            .context("Message not found")?;

        // 2. 构建操作消息并发布到 Kafka
        let store_request = MessageOperationBuilder::build_recall_request(&cmd)
            .context("Failed to build recall request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish recall operation to Kafka")?;

        // 发布领域事件
        let event = MessageRecalledEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
            reason: cmd.reason.clone(),
            new_state: MessageFsmState::Recalled,
        };
        self.event_publisher.publish_recalled(&event).await?;

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, operator_id = %cmd.base.operator_id))]
    pub async fn handle_edit(&self, cmd: EditMessageCommand) -> Result<()> {
        // 1. 查询原消息（用于权限验证和快速失败）
        // 策略：先查 Reader（已持久化的消息），如果查不到，再查 WAL（刚发送但未持久化的消息）
        let mut original_message = self
            .message_repo
            .find_by_id(&cmd.base.message_id)
            .await?;

        // 如果 Reader 查询不到，尝试从 WAL 查询（解决时序问题：消息刚发送但未持久化）
        if original_message.is_none() {
            tracing::debug!(
                message_id = %cmd.base.message_id,
                "Message not found in Reader, trying WAL fallback"
            );
            if let Some(wal_repo) = &self.wal_repository {
                match wal_repo.find_by_message_id(&cmd.base.message_id).await {
                    Ok(Some(proto_message)) => {
                        tracing::info!(
                            message_id = %cmd.base.message_id,
                            "✅ Found message in WAL, using for permission validation"
                        );
                    // 将 Proto Message 转换为 Domain Message
                    use crate::domain::model::MessageFsmState;
                    use chrono::{DateTime, Utc};
                    
                    let fsm_state = if proto_message.is_recalled {
                        MessageFsmState::Recalled
                    } else if proto_message.status == flare_proto::common::MessageStatus::DeletedHard as i32 {
                        MessageFsmState::DeletedHard
                    } else {
                        MessageFsmState::from_str(
                            proto_message.extra.get("message_fsm_state")
                                .map(|s| s.as_str())
                                .unwrap_or("SENT")
                        ).unwrap_or(MessageFsmState::Sent)
                    };

                    let timestamp = proto_message.timestamp
                        .map(|ts| {
                            DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
                                .unwrap_or_else(Utc::now)
                        })
                        .unwrap_or_else(Utc::now);

                    let content_bytes = proto_message.content
                        .as_ref()
                        .map(|c| {
                            use prost::Message;
                            let mut buf = Vec::new();
                            c.encode(&mut buf).unwrap_or_default();
                            buf
                        })
                        .unwrap_or_default();

                    original_message = Some(Message {
                        server_id: proto_message.server_id.clone(),
                        conversation_id: proto_message.conversation_id.clone(),
                        sender_id: proto_message.sender_id.clone(),
                        content: content_bytes,
                        timestamp,
                        fsm_state,
                        fsm_state_changed_at: timestamp,
                        edit_version: proto_message.extra.get("current_edit_version")
                            .and_then(|v| v.parse::<i32>().ok())
                            .unwrap_or(0),
                        edit_history: vec![],
                        updated_at: timestamp,
                    });
                    }
                    Ok(None) => {
                        tracing::debug!(
                            message_id = %cmd.base.message_id,
                            "Message not found in WAL either"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            message_id = %cmd.base.message_id,
                            error = %e,
                            "Failed to query WAL for message"
                        );
                    }
                }
            } else {
                tracing::debug!(
                    message_id = %cmd.base.message_id,
                    "WAL repository not configured, cannot use fallback"
                );
            }
        }

        // 如果 Reader 和 WAL 都查询不到，可能是时序问题（消息刚发送但未持久化）
        // 或者 WAL 没有启用
        let original_message = match original_message {
            Some(msg) => msg,
            None => {
                let wal_configured = self.wal_repository.is_some();
                if !wal_configured {
                    tracing::error!(
                        message_id = %cmd.base.message_id,
                        "❌ WAL not configured (wal_hash_key is None). Cannot validate edit permissions. Please configure WAL or wait for message to be persisted."
                    );
                    return Err(anyhow::anyhow!(
                        "Message not found and WAL not configured. Cannot validate edit permissions. Please configure WAL (MESSAGE_ORCHESTRATOR_WAL_HASH_KEY) or wait for message to be persisted."
                    ));
                } else {
                    tracing::warn!(
                        message_id = %cmd.base.message_id,
                        "⚠️ Message not found in Reader or WAL. This may be a timing issue (message just sent but not yet persisted). Please wait a moment and try again."
                    );
                    return Err(anyhow::anyhow!(
                        "Message not found (checked both Reader and WAL). This may be a timing issue. Please wait a moment and try again."
                    ));
                }
            }
        };

        // 2. 验证权限（只有发送者可以编辑）- 快速失败，立即返回错误给客户端
        if original_message.sender_id != cmd.base.operator_id {
            return Err(anyhow::anyhow!(
                "Permission denied: Only message sender can edit message. \
                 Sender: {}, Operator: {}",
                original_message.sender_id,
                cmd.base.operator_id
            ));
        }

        // 2.1. 如果命令中没有 conversation_id，从查询到的消息中获取
        let mut cmd = cmd;
        if cmd.base.conversation_id.is_empty() {
            cmd.base.conversation_id = original_message.conversation_id.clone();
        }

        // 3. 构建操作消息并发布到 Kafka（权限已验证，Writer 只负责写入）
        let store_request = MessageOperationBuilder::build_edit_request(&cmd)
            .context("Failed to build edit request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish edit operation to Kafka")?;

        // 4. 发布领域事件（用于推送通知）
        let event = MessageEditedEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
            edit_version: original_message.edit_version + 1, // 使用查询到的版本号 + 1
            new_state: MessageFsmState::Edited,
            reason: cmd.reason.clone(),
        };
        self.event_publisher.publish_edited(&event).await?;

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, operator_id = %cmd.base.operator_id))]
    pub async fn handle_delete(&self, cmd: DeleteMessageCommand) -> Result<()> {
        match cmd.delete_type {
            DeleteType::Hard => {
                let store_request = MessageOperationBuilder::build_delete_request(&cmd)
                    .context("Failed to build delete request")?;
                
                self.kafka_publisher
                    .publish_storage(store_request)
                    .await
                    .context("Failed to publish delete operation to Kafka")?;

                let event = MessageDeletedEvent {
                    base: MessageOperationEvent {
                        message_id: cmd.base.message_id.clone(),
                        conversation_id: cmd.base.conversation_id.clone(),
                        operator_id: cmd.base.operator_id.clone(),
                        timestamp: cmd.base.timestamp,
                        tenant_id: cmd.base.tenant_id.clone(),
                    },
                    delete_type: "HARD".to_string(),
                    new_state: Some(MessageFsmState::DeletedHard),
                    target_user_id: None,
                };
                self.event_publisher.publish_deleted(&event).await?;
            }
            DeleteType::Soft => {
                let store_request = MessageOperationBuilder::build_delete_request(&cmd)
                    .context("Failed to build delete request")?;
                
                self.kafka_publisher
                    .publish_storage(store_request)
                    .await
                    .context("Failed to publish soft delete operation to Kafka")?;

                let event = MessageDeletedEvent {
                    base: MessageOperationEvent {
                        message_id: cmd.base.message_id.clone(),
                        conversation_id: cmd.base.conversation_id.clone(),
                        operator_id: cmd.base.operator_id.clone(),
                        timestamp: cmd.base.timestamp,
                        tenant_id: cmd.base.tenant_id.clone(),
                    },
                    delete_type: "SOFT".to_string(),
                    new_state: None,
                    target_user_id: cmd.target_user_id.clone(),
                };
                self.event_publisher.publish_deleted(&event).await?;
            }
        }

        Ok(())
    }

    #[instrument(skip(self), fields(operator_id = %cmd.base.operator_id))]
    pub async fn handle_read(&self, cmd: ReadMessageCommand) -> Result<()> {
        let event = MessageReadEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
            message_ids: cmd.message_ids.clone(),
            read_at: cmd.read_at.unwrap_or_else(|| Utc::now()),
        };
        self.event_publisher.publish_read(&event).await?;

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, emoji = %cmd.emoji))]
    pub async fn handle_add_reaction(&self, cmd: AddReactionCommand) -> Result<i32> {
        // 1. 查询消息以获取当前反应计数（如果需要）
        // 注意：反应计数应该由读模型维护，这里返回占位值
        // 实际计数应该在查询时从 message_reactions 表统计
        
        // 2. 构建操作消息并发布到 Kafka（storage-writer 会保存到 message_reactions 表）
        let store_request = MessageOperationBuilder::build_add_reaction_request(&cmd)
            .context("Failed to build add reaction request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish add reaction operation to Kafka")?;

        // 3. 发布领域事件
        let event = MessageReactionAddedEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
            emoji: cmd.emoji.clone(),
            count: 0, // 由读模型计算
        };
        self.event_publisher.publish_reaction_added(&event).await?;

        // 返回占位计数（实际计数应该从读模型查询）
        Ok(0)
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, emoji = %cmd.emoji))]
    pub async fn handle_remove_reaction(&self, cmd: RemoveReactionCommand) -> Result<i32> {
        // 1. 查询消息以获取当前反应计数（如果需要）
        // 注意：反应计数应该由读模型维护，这里返回占位值
        // 实际计数应该在查询时从 message_reactions 表统计
        
        // 2. 构建操作消息并发布到 Kafka（storage-writer 会保存到 message_reactions 表）
        let store_request = MessageOperationBuilder::build_remove_reaction_request(&cmd)
            .context("Failed to build remove reaction request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish remove reaction operation to Kafka")?;

        // 3. 发布领域事件
        let event = MessageReactionRemovedEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
            emoji: cmd.emoji.clone(),
            count: 0, // 由读模型计算
        };
        self.event_publisher.publish_reaction_removed(&event).await?;

        // 返回占位计数（实际计数应该从读模型查询）
        Ok(0)
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_pin(&self, cmd: PinMessageCommand) -> Result<()> {
        // 1. 构建操作消息并发布到 Kafka（storage-writer 会保存到 pinned_messages 表）
        let store_request = MessageOperationBuilder::build_pin_request(&cmd)
            .context("Failed to build pin request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish pin operation to Kafka")?;

        // 2. 发布领域事件
        let event = MessagePinnedEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
        };
        self.event_publisher.publish_pinned(&event).await?;

        Ok(())
    }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    pub async fn handle_unpin(&self, cmd: UnpinMessageCommand) -> Result<()> {
        // 1. 构建操作消息并发布到 Kafka（storage-writer 会保存到 pinned_messages 表）
        let store_request = MessageOperationBuilder::build_unpin_request(&cmd)
            .context("Failed to build unpin request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish unpin operation to Kafka")?;

        // 2. 发布领域事件
        let event = MessageUnpinnedEvent {
            base: MessageOperationEvent {
                message_id: cmd.base.message_id.clone(),
                conversation_id: cmd.base.conversation_id.clone(),
                operator_id: cmd.base.operator_id.clone(),
                timestamp: cmd.base.timestamp,
                tenant_id: cmd.base.tenant_id.clone(),
            },
        };
        self.event_publisher.publish_unpinned(&event).await?;

        Ok(())
    }

    // Favorite/Unfavorite 功能暂未实现
    // #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    // pub async fn handle_favorite(&self, cmd: FavoriteMessageCommand) -> Result<()> {
    //     let event = MessageFavoritedEvent {
    //         base: MessageOperationEvent {
    //             message_id: cmd.base.message_id.clone(),
    //             conversation_id: cmd.base.conversation_id.clone(),
    //             operator_id: cmd.base.operator_id.clone(),
    //             timestamp: cmd.base.timestamp,
    //             tenant_id: cmd.base.tenant_id.clone(),
    //         },
    //         tags: cmd.tags.clone(),
    //     };
    //     self.event_publisher.publish_favorited(&event).await?;
    //
    //     Ok(())
    // }

    // #[instrument(skip(self), fields(message_id = %cmd.base.message_id))]
    // pub async fn handle_unfavorite(&self, cmd: UnfavoriteMessageCommand) -> Result<()> {
    //     let event = MessageUnfavoritedEvent {
    //         base: MessageOperationEvent {
    //             message_id: cmd.base.message_id.clone(),
    //             conversation_id: cmd.base.conversation_id.clone(),
    //             operator_id: cmd.base.operator_id.clone(),
    //             timestamp: cmd.base.timestamp,
    //             tenant_id: cmd.base.tenant_id.clone(),
    //         },
    //     };
    //     self.event_publisher.publish_unfavorited(&event).await?;
    //
    //     Ok(())
    // }

    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, mark_type = %cmd.mark_type))]
    pub async fn handle_mark(&self, cmd: MarkMessageCommand) -> Result<()> {
        // 1. 构建操作消息并发布到 Kafka（storage-writer 会保存到 marked_messages 表）
        let store_request = MessageOperationBuilder::build_mark_request(&cmd)
            .context("Failed to build mark request")?;
        
        self.kafka_publisher
            .publish_operation(store_request)
            .await
            .context("Failed to publish mark operation to Kafka")?;

        // 2. 发布领域事件（如果需要）
        // 注意：目前 EventPublisher trait 中没有 publish_marked 方法
        // 如果需要，可以在 EventPublisher trait 中添加

        Ok(())
    }

    /// 取消标记消息（业务逻辑）
    #[instrument(skip(self), fields(message_id = %cmd.base.message_id, user_id = %cmd.user_id))]
    pub async fn handle_unmark(&self, cmd: UnmarkMessageCommand) -> Result<()> {
        // 1. 查询消息的当前标记信息
        let _message = self
            .message_repo
            .find_by_id(&cmd.base.message_id)
            .await?
            .context("Message not found")?;

        // 2. 构建操作消息并发布到 Kafka
        // 注意：取消标记是用户维度的操作，不需要 Kafka 推送，但需要持久化到数据库
        // TODO: 添加 MessageOperationBuilder::build_unmark_request
        // let store_request = MessageOperationBuilder::build_unmark_request(&cmd)
        //     .context("Failed to build unmark request")?;
        
        // self.kafka_publisher
        //     .publish_operation(store_request)
        //     .await
        //     .context("Failed to publish unmark operation to Kafka")?;

        // 注意：目前取消标记操作的属性更新逻辑在 handler 中直接调用 Storage Reader
        // 未来应该通过 MessageOperationBuilder 构建操作消息，发布到 Kafka，由 Storage Writer 处理
        // 这样可以保证一致性，并支持事件驱动架构

        Ok(())
    }
}

