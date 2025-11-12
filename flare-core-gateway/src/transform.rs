use anyhow::{Context, Result};
use flare_proto::communication_core::{
    BusinessType, Message as CoreMessage, MessageContent, MessageType, Notification,
    NotificationType, PushOptions, notification_content,
};
use flare_proto::push::Notification as PushNotification;
use flare_proto::storage::Message as StorageMessage;
use prost::Message as _;

pub fn core_push_options_to_push(options: &Option<PushOptions>) -> flare_proto::push::PushOptions {
    options
        .as_ref()
        .map(|opts| flare_proto::push::PushOptions {
            require_online: opts.require_online,
            persist_if_offline: opts.persist_if_offline,
            priority: opts.priority,
            metadata: opts.metadata.clone(),
        })
        .unwrap_or_default()
}

pub fn core_notification_to_push(notification: &Notification) -> PushNotification {
    let notification_type =
        NotificationType::from_i32(notification.r#type).unwrap_or(NotificationType::Unspecified);

    let (title, body, data) = match notification
        .content
        .as_ref()
        .and_then(|wrapper| wrapper.content.as_ref())
    {
        Some(notification_content::Content::System(system)) => (
            system.title.clone(),
            system.body.clone(),
            system.data.clone(),
        ),
        Some(notification_content::Content::Conversation(conv)) => (
            "conversation".into(),
            conv.summary.clone(),
            Default::default(),
        ),
        Some(notification_content::Content::Status(status)) => (
            "status".into(),
            format!("{}: {}", status.user_id, status.status),
            Default::default(),
        ),
        Some(notification_content::Content::Group(group)) => {
            (group.title.clone(), group.body.clone(), group.data.clone())
        }
        Some(notification_content::Content::Friend(friend)) => (
            "friend".into(),
            format!("{}: {}", friend.user_id, friend.action),
            Default::default(),
        ),
        Some(notification_content::Content::Security(security)) => (
            security.title.clone(),
            security.body.clone(),
            security.data.clone(),
        ),
        Some(notification_content::Content::Device(device)) => (
            "device".into(),
            format!("{}: {}", device.device_id, device.action),
            device.data.clone(),
        ),
        Some(notification_content::Content::Custom(custom)) => (
            custom.r#type.clone(),
            custom.json_payload.clone(),
            custom.data.clone(),
        ),
        None => (
            notification_type.as_str_name().to_lowercase(),
            String::new(),
            Default::default(),
        ),
    };

    PushNotification {
        title,
        body,
        data,
        metadata: notification.metadata.clone(),
    }
}

pub fn core_to_storage_message(message: &CoreMessage) -> Result<StorageMessage> {
    let message_type = MessageType::from_i32(message.message_type).unwrap_or(MessageType::Custom);
    let content_type = message_type_label(message_type).to_string();
    let payload = encode_structured_content(&message.content)?;

    Ok(StorageMessage {
        id: message.id.clone(),
        session_id: message.session_id.clone(),
        sender_id: message.sender_id.clone(),
        sender_type: message.sender_type.clone(),
        receiver_ids: message.receiver_ids.clone(),
        receiver_id: message.receiver_id.clone(),
        content: payload,
        content_type,
        timestamp: message.timestamp.clone(),
        status: message.status.clone(),
        extra: message.extra.clone(),
        business_type: business_type_label(message.business_type),
        session_type: message.session_type.clone(),
        is_recalled: message.is_recalled,
        recalled_at: message.recalled_at.clone(),
        recall_reason: String::new(),
        is_burn_after_read: message.is_burn_after_read,
        burn_after_seconds: message.burn_after_seconds,
        read_by: Vec::new(),
        visibility: Default::default(),
        operations: Vec::new(),
        tenant: message.tenant.clone(),
        attachments: message.attachments.clone(),
        audit: message.audit.clone(),
        message_type: message_type as i32,
        structured_content: message.content.clone(),
    })
}

pub fn storage_to_core_message(message: StorageMessage) -> CoreMessage {
    CoreMessage {
        id: message.id,
        session_id: message.session_id,
        sender_id: message.sender_id,
        sender_type: message.sender_type,
        receiver_ids: message.receiver_ids,
        receiver_id: message.receiver_id,
        content: message.structured_content,
        timestamp: message.timestamp,
        message_type: message.message_type,
        business_type: parse_business_type(&message.business_type) as i32,
        session_type: message.session_type,
        status: message.status,
        extra: message.extra,
        is_recalled: message.is_recalled,
        recalled_at: message.recalled_at,
        is_burn_after_read: message.is_burn_after_read,
        burn_after_seconds: message.burn_after_seconds,
        tenant: message.tenant,
        attachments: message.attachments,
        audit: message.audit,
    }
}

pub fn core_message_to_push_bytes(message: &CoreMessage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    message
        .encode(&mut buf)
        .context("encode push message payload")?;
    Ok(buf)
}

fn encode_structured_content(content: &Option<MessageContent>) -> Result<Vec<u8>> {
    if let Some(content) = content {
        let mut buf = Vec::new();
        content
            .encode(&mut buf)
            .context("encode structured content")?;
        Ok(buf)
    } else {
        Ok(Vec::new())
    }
}

fn message_type_label(message_type: MessageType) -> &'static str {
    match message_type {
        MessageType::Unspecified => "unspecified",
        MessageType::Text => "text",
        MessageType::RichText => "rich_text",
        MessageType::Image => "image",
        MessageType::Video => "video",
        MessageType::Audio => "audio",
        MessageType::File => "file",
        MessageType::Sticker => "sticker",
        MessageType::Location => "location",
        MessageType::Card => "card",
        MessageType::Command => "command",
        MessageType::Event => "event",
        MessageType::System => "system",
        MessageType::Custom => "custom",
    }
}

fn business_type_label(value: i32) -> String {
    match BusinessType::from_i32(value).unwrap_or(BusinessType::Other) {
        BusinessType::SingleChat => "im".into(),
        BusinessType::GroupChat => "group".into(),
        BusinessType::CustomerService => "cs".into(),
        BusinessType::AiBot => "ai".into(),
        BusinessType::Other => "other".into(),
    }
}

fn parse_business_type(value: &str) -> BusinessType {
    match value.to_lowercase().as_str() {
        "im" | "single_chat" => BusinessType::SingleChat,
        "group" | "group_chat" => BusinessType::GroupChat,
        "cs" | "customer_service" => BusinessType::CustomerService,
        "ai" | "ai_bot" => BusinessType::AiBot,
        _ => BusinessType::Other,
    }
}
