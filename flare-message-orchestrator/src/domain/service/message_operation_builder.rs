//! 消息操作消息构建器

use anyhow::{Context, Result};
use flare_proto::common::{
    Message, MessageContent, MessageType, NotificationContent, MessageSource,
    EditOperationData,
};
use flare_proto::storage::StoreMessageRequest;
use prost::Message as ProstMessage;
use uuid::Uuid;

use crate::application::commands::{
    AddReactionCommand, DeleteMessageCommand, DeleteType, EditMessageCommand,
    MarkMessageCommand, MessageOperationCommand, PinMessageCommand, RecallMessageCommand,
    RemoveReactionCommand, UnpinMessageCommand,
};

/// 消息操作消息构建器
pub struct MessageOperationBuilder;

impl MessageOperationBuilder {
    /// 构建撤回消息的 StoreMessageRequest
    pub fn build_recall_request(
        cmd: &RecallMessageCommand,
    ) -> Result<StoreMessageRequest> {
        let operation_id = format!("op-{}", Uuid::new_v4());
        
        // 构建操作通知消息
        let notification = NotificationContent {
            title: "消息撤回".to_string(),
            body: cmd.reason.clone().unwrap_or_else(|| "对方撤回了一条消息".to_string()),
            notification_type: "message_operation".to_string(),
            data: {
                let mut data = std::collections::HashMap::new();
                data.insert("operation_type".to_string(), "OPERATION_TYPE_RECALL".to_string());
                data.insert("target_message_id".to_string(), cmd.base.message_id.clone());
                data.insert("operator_id".to_string(), cmd.base.operator_id.clone());
                if let Some(reason) = &cmd.reason {
                    data.insert("reason".to_string(), reason.clone());
                }
                data
            },
            target_user_ids: vec![],
            target_role_id: "".to_string(),
            notify_all: false,
            persistent: true,
            show_in_list: true,
            show_badge: false,
            play_sound: false,
            extensions: vec![],
        };

        let mut message = Message::default();
        message.server_id = operation_id.clone();
        message.conversation_id = cmd.base.conversation_id.clone();
        message.client_msg_id = format!("client-op-{}", Uuid::new_v4());
        message.sender_id = cmd.base.operator_id.clone();
        message.source = MessageSource::System as i32;
        message.seq = 0;
        message.timestamp = Some(prost_types::Timestamp {
            seconds: cmd.base.timestamp.timestamp(),
            nanos: cmd.base.timestamp.timestamp_subsec_nanos() as i32,
        });
        message.conversation_type = 0;
        message.message_type = MessageType::Notification as i32;
        message.business_type = "message_operation".to_string();
        message.receiver_id = String::new();
        message.channel_id = cmd.base.conversation_id.clone();
        message.content = Some(MessageContent {
            content: Some(
                flare_proto::common::message_content::Content::Notification(notification),
            ),
            extensions: vec![],
        });
        message.attributes = {
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("operation_type".to_string(), "OPERATION_TYPE_RECALL".to_string());
            attrs.insert("target_message_id".to_string(), cmd.base.message_id.clone());
            attrs
        };
        message.extra = {
            let mut extra = std::collections::HashMap::new();
            extra.insert("is_operation_message".to_string(), "true".to_string());
            extra.insert("operation_type".to_string(), "OPERATION_TYPE_RECALL".to_string());
            extra
        };
        message.tags = vec![];

        Ok(StoreMessageRequest {
            conversation_id: cmd.base.conversation_id.clone(),
            message: Some(message),
            sync: false,
            context: None,
            tenant: None,
            tags: std::collections::HashMap::new(),
        })
    }

    /// 构建编辑消息的 StoreMessageRequest
    pub fn build_edit_request(cmd: &EditMessageCommand) -> Result<StoreMessageRequest> {
        use flare_proto::common::{
            MessageOperation, OperationType,
            message_operation::OperationData,
        };
        use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

        // 1. 解析 new_content 为 MessageContent
        let new_content = flare_proto::common::MessageContent::decode(cmd.new_content.as_slice())
            .context("Failed to decode new_content as MessageContent")?;

        // 2. 构建 MessageOperation
        // 将 MessageContent 序列化为 Vec<u8>
        let new_content_bytes = {
            let mut buf = Vec::new();
            new_content.encode(&mut buf).context("Failed to encode new_content")?;
            buf
        };
        
        let edit_data = EditOperationData {
            new_content: new_content_bytes,
            edit_version: 0, // 版本号由 Writer 在持久化时确定
            reason: cmd.reason.clone().unwrap_or_default(),
            show_edited_mark: true,
        };

        let operation = MessageOperation {
            operation_type: OperationType::Edit as i32,
            target_message_id: cmd.base.message_id.clone(),
            operator_id: cmd.base.operator_id.clone(),
            timestamp: Some(prost_types::Timestamp {
                seconds: cmd.base.timestamp.timestamp(),
                nanos: cmd.base.timestamp.timestamp_subsec_nanos() as i32,
            }),
            operation_data: Some(OperationData::Edit(edit_data)),
            metadata: std::collections::HashMap::new(),
            notice_text: String::new(),
            show_notice: false,
            target_user_id: String::new(),
        };

        // 3. 序列化 MessageOperation 并 base64 编码
        let mut operation_bytes = Vec::new();
        operation.encode(&mut operation_bytes)
            .context("Failed to encode MessageOperation")?;
        let operation_base64 = BASE64.encode(&operation_bytes);

        // 4. 构建 NotificationContent（与 SDK 格式一致）
        let notification = NotificationContent {
            title: "消息编辑".to_string(),
            body: "消息已编辑".to_string(),
            notification_type: "message_operation".to_string(),
            data: {
                let mut data = std::collections::HashMap::new();
                data.insert("operation_data".to_string(), operation_base64);
                data.insert("operation_type".to_string(), format!("{}", OperationType::Edit as i32));
                data.insert("target_message_id".to_string(), cmd.base.message_id.clone());
                data.insert("operator_id".to_string(), cmd.base.operator_id.clone());
                if let Some(reason) = &cmd.reason {
                    data.insert("reason".to_string(), reason.clone());
                }
                data
            },
            target_user_ids: vec![],
            target_role_id: "".to_string(),
            notify_all: false,
            persistent: true,
            show_in_list: false, // 操作消息不在消息列表中显示
            show_badge: false,
            play_sound: false,
            extensions: vec![],
        };

        // 5. 构建操作消息
        let operation_id = format!("op-{}", Uuid::new_v4());
        let mut message = Message::default();
        message.server_id = operation_id.clone();
        message.conversation_id = cmd.base.conversation_id.clone();
        message.client_msg_id = format!("client-op-{}", Uuid::new_v4());
        message.sender_id = cmd.base.operator_id.clone();
        message.source = MessageSource::System as i32;
        message.seq = 0;
        message.timestamp = Some(prost_types::Timestamp {
            seconds: cmd.base.timestamp.timestamp(),
            nanos: cmd.base.timestamp.timestamp_subsec_nanos() as i32,
        });
        message.conversation_type = 0;
        message.message_type = MessageType::Notification as i32;
        message.business_type = "message_operation".to_string();
        message.receiver_id = String::new();
        message.channel_id = cmd.base.conversation_id.clone();
        message.content = Some(MessageContent {
            content: Some(
                flare_proto::common::message_content::Content::Notification(notification),
            ),
            extensions: vec![],
        });
        message.attributes = {
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("operation_type".to_string(), "OPERATION_TYPE_EDIT".to_string());
            attrs.insert("target_message_id".to_string(), cmd.base.message_id.clone());
            attrs
        };
        message.extra = {
            let mut extra = std::collections::HashMap::new();
            extra.insert("is_operation_message".to_string(), "true".to_string());
            extra.insert("operation_type".to_string(), "OPERATION_TYPE_EDIT".to_string());
            extra
        };
        message.tags = vec![];

        Ok(StoreMessageRequest {
            conversation_id: cmd.base.conversation_id.clone(),
            message: Some(message),
            sync: false,
            context: None,
            tenant: Some(flare_proto::common::TenantContext {
                tenant_id: cmd.base.tenant_id.clone(),
                business_type: String::new(),
                environment: String::new(),
                organization_id: String::new(),
                labels: std::collections::HashMap::new(),
                attributes: std::collections::HashMap::new(),
            }),
            tags: std::collections::HashMap::new(),
        })
    }

    /// 构建删除消息的 StoreMessageRequest
    pub fn build_delete_request(cmd: &DeleteMessageCommand) -> Result<StoreMessageRequest> {
        let operation_id = format!("op-{}", Uuid::new_v4());

        let notification = NotificationContent {
            title: "消息删除".to_string(),
            body: match cmd.delete_type {
                DeleteType::Hard => "消息已删除".to_string(),
                DeleteType::Soft => "消息已隐藏".to_string(),
            },
            notification_type: "message_operation".to_string(),
            data: {
                let mut data = std::collections::HashMap::new();
                data.insert("operation_type".to_string(), "OPERATION_TYPE_DELETE".to_string());
                data.insert("target_message_id".to_string(), cmd.base.message_id.clone());
                data.insert("operator_id".to_string(), cmd.base.operator_id.clone());
                data.insert("delete_type".to_string(), format!("{:?}", cmd.delete_type));
                if let Some(reason) = &cmd.reason {
                    data.insert("reason".to_string(), reason.clone());
                }
                if let Some(target_user_id) = &cmd.target_user_id {
                    data.insert("target_user_id".to_string(), target_user_id.clone());
                }
                data
            },
            target_user_ids: vec![],
            target_role_id: "".to_string(),
            notify_all: false,
            persistent: true,
            show_in_list: true,
            show_badge: false,
            play_sound: false,
            extensions: vec![],
        };

        let mut message = Message::default();
        message.server_id = operation_id.clone();
        message.conversation_id = cmd.base.conversation_id.clone();
        message.client_msg_id = format!("client-op-{}", Uuid::new_v4());
        message.sender_id = cmd.base.operator_id.clone();
        message.source = MessageSource::System as i32;
        message.seq = 0;
        message.timestamp = Some(prost_types::Timestamp {
            seconds: cmd.base.timestamp.timestamp(),
            nanos: cmd.base.timestamp.timestamp_subsec_nanos() as i32,
        });
        message.conversation_type = 0;
        message.message_type = MessageType::Notification as i32;
        message.business_type = "message_operation".to_string();
        message.receiver_id = String::new();
        message.channel_id = cmd.base.conversation_id.clone();
        message.content = Some(MessageContent {
            content: Some(
                flare_proto::common::message_content::Content::Notification(notification),
            ),
            extensions: vec![],
        });
        message.attributes = {
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("operation_type".to_string(), "OPERATION_TYPE_DELETE".to_string());
            attrs.insert("target_message_id".to_string(), cmd.base.message_id.clone());
            attrs.insert("delete_type".to_string(), format!("{:?}", cmd.delete_type));
            attrs
        };
        message.extra = {
            let mut extra = std::collections::HashMap::new();
            extra.insert("is_operation_message".to_string(), "true".to_string());
            extra.insert("operation_type".to_string(), "OPERATION_TYPE_DELETE".to_string());
            extra
        };
        message.tags = vec![];

        Ok(StoreMessageRequest {
            conversation_id: cmd.base.conversation_id.clone(),
            message: Some(message),
            sync: false,
            context: None,
            tenant: None,
            tags: std::collections::HashMap::new(),
        })
    }

    /// 构建置顶消息的 StoreMessageRequest
    pub fn build_pin_request(cmd: &PinMessageCommand) -> Result<StoreMessageRequest> {
        Self::build_operation_request(
            &cmd.base,
            "OPERATION_TYPE_PIN",
            "消息置顶",
            "消息已置顶",
            None,
        )
    }

    /// 构建取消置顶消息的 StoreMessageRequest
    pub fn build_unpin_request(cmd: &UnpinMessageCommand) -> Result<StoreMessageRequest> {
        Self::build_operation_request(
            &cmd.base,
            "OPERATION_TYPE_UNPIN",
            "取消置顶",
            "消息已取消置顶",
            None,
        )
    }

    /// 构建添加反应操作的 StoreMessageRequest
    pub fn build_add_reaction_request(cmd: &AddReactionCommand) -> Result<StoreMessageRequest> {
        let mut extra_data = std::collections::HashMap::new();
        extra_data.insert("emoji".to_string(), cmd.emoji.clone());
        Self::build_operation_request(
            &cmd.base,
            "OPERATION_TYPE_REACTION_ADD",
            "添加反应",
            "添加了反应",
            Some(extra_data),
        )
    }

    /// 构建移除反应操作的 StoreMessageRequest
    pub fn build_remove_reaction_request(cmd: &RemoveReactionCommand) -> Result<StoreMessageRequest> {
        let mut extra_data = std::collections::HashMap::new();
        extra_data.insert("emoji".to_string(), cmd.emoji.clone());
        Self::build_operation_request(
            &cmd.base,
            "OPERATION_TYPE_REACTION_REMOVE",
            "移除反应",
            "移除了反应",
            Some(extra_data),
        )
    }

    /// 构建标记消息的 StoreMessageRequest
    pub fn build_mark_request(cmd: &MarkMessageCommand) -> Result<StoreMessageRequest> {
        let mut extra_data = std::collections::HashMap::new();
        extra_data.insert("mark_type".to_string(), cmd.mark_type.to_string());
        if let Some(ref mark_value) = cmd.mark_value {
            extra_data.insert("mark_value".to_string(), mark_value.clone());
        }
        Self::build_operation_request(
            &cmd.base,
            "OPERATION_TYPE_MARK",
            "标记消息",
            "消息已标记",
            Some(extra_data),
        )
    }

    /// 通用方法：构建操作消息的 StoreMessageRequest
    fn build_operation_request(
        base: &MessageOperationCommand,
        operation_type: &str,
        title: &str,
        body: &str,
        extra_data: Option<std::collections::HashMap<String, String>>,
    ) -> Result<StoreMessageRequest> {
        let operation_id = format!("op-{}", Uuid::new_v4());
        
        let mut data = std::collections::HashMap::new();
        data.insert("operation_type".to_string(), operation_type.to_string());
        data.insert("target_message_id".to_string(), base.message_id.clone());
        data.insert("operator_id".to_string(), base.operator_id.clone());
        if let Some(extra) = extra_data {
            data.extend(extra);
        }

        let notification = NotificationContent {
            title: title.to_string(),
            body: body.to_string(),
            notification_type: "message_operation".to_string(),
            data: data.clone(),
            target_user_ids: Vec::new(),
            target_role_id: String::new(),
            notify_all: false,
            persistent: true,
            show_in_list: false,
            show_badge: false,
            play_sound: false,
            extensions: Vec::new(),
        };

        let mut message = Message::default();
        message.server_id = operation_id.clone();
        message.conversation_id = base.conversation_id.clone();
        message.client_msg_id = format!("client-op-{}", Uuid::new_v4());
        message.sender_id = base.operator_id.clone();
        message.source = MessageSource::System as i32;
        message.seq = 0;
        message.timestamp = Some(prost_types::Timestamp {
            seconds: base.timestamp.timestamp(),
            nanos: base.timestamp.timestamp_subsec_nanos() as i32,
        });
        message.conversation_type = 0;
        message.message_type = MessageType::Notification as i32;
        message.business_type = "message_operation".to_string();
        message.receiver_id = String::new();
        message.channel_id = base.conversation_id.clone();
        message.content = Some(MessageContent {
            content: Some(
                flare_proto::common::message_content::Content::Notification(notification),
            ),
            extensions: vec![],
        });
        message.attributes = {
            let mut attrs = std::collections::HashMap::new();
            attrs.insert("operation_type".to_string(), operation_type.to_string());
            attrs.insert("target_message_id".to_string(), base.message_id.clone());
            attrs
        };
        message.extra = {
            let mut extra = std::collections::HashMap::new();
            extra.insert("is_operation_message".to_string(), "true".to_string());
            extra.insert("operation_type".to_string(), operation_type.to_string());
            extra
        };
        message.tags = vec![];

        Ok(StoreMessageRequest {
            conversation_id: base.conversation_id.clone(),
            message: Some(message),
            sync: false,
            context: None,
            tenant: None,
            tags: std::collections::HashMap::new(),
        })
    }
}

