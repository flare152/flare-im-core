use flare_proto::communication_core::MessageType;
use flare_proto::storage::Message as StorageMessage;

/// 统一的消息类型推断与归一化。
pub struct MessageProfile {
    message_type: MessageType,
}

impl MessageProfile {
    pub fn ensure(message: &mut StorageMessage) -> Self {
        let detected = MessageType::from_i32(message.message_type)
            .unwrap_or_else(|| infer_from_content_type(&message.content_type));

        message.message_type = detected as i32;

        if message.content_type.is_empty() {
            message.content_type = message_type_label(detected).to_string();
        }

        message
            .extra
            .entry("message_type".into())
            .or_insert_with(|| message_type_label(detected).to_string());

        MessageProfile {
            message_type: detected,
        }
    }

    pub fn message_type(&self) -> MessageType {
        self.message_type
    }

    pub fn message_type_label(&self) -> &'static str {
        message_type_label(self.message_type)
    }
}

fn infer_from_content_type(raw: &str) -> MessageType {
    match raw.trim().to_lowercase().as_str() {
        "text/plain" | "text" | "plain_text" => MessageType::Text,
        "markdown" | "text/markdown" | "rich_text" | "rich-text" => MessageType::RichText,
        "image" | "image/png" | "image/jpeg" | "image/jpg" => MessageType::Image,
        "video" | "video/mp4" | "video/mpeg" => MessageType::Video,
        "audio" | "audio/aac" | "audio/mpeg" | "voice" => MessageType::Audio,
        "file" | "application/octet-stream" | "application/pdf" | "application/zip" => {
            MessageType::File
        }
        "sticker" | "emoji" | "gif" => MessageType::Sticker,
        "location" | "geo" | "geolocation" => MessageType::Location,
        "card" | "share_card" | "invite_card" => MessageType::Card,
        "command" | "cmd" => MessageType::Command,
        "event" => MessageType::Event,
        "system" | "system_message" => MessageType::System,
        _ => MessageType::Custom,
    }
}

fn message_type_label(ty: MessageType) -> &'static str {
    match ty {
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

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::storage::Message;

    fn message(content_type: &str, message_type: i32) -> Message {
        Message {
            content_type: content_type.to_string(),
            message_type,
            ..Message::default()
        }
    }

    #[test]
    fn infer_from_content_type_text() {
        let mut msg = message("text/plain", 0);
        let profile = MessageProfile::ensure(&mut msg);
        assert_eq!(profile.message_type(), MessageType::Text);
        assert_eq!(msg.content_type, "text");
    }

    #[test]
    fn preserve_explicit_type() {
        let mut msg = message("custom", MessageType::Card as i32);
        let profile = MessageProfile::ensure(&mut msg);
        assert_eq!(profile.message_type(), MessageType::Card);
        assert_eq!(msg.content_type, "custom");
    }
}
