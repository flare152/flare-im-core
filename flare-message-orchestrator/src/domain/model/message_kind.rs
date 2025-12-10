use flare_proto::common::{MessageType, Message as StorageMessage};

/// 消息处理类型（用于决定是否需要持久化）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageProcessingType {
    /// 普通消息：需要持久化+推送
    Normal,
    /// 通知消息：仅推送，不持久化，离线舍弃
    Notification,
}

/// 统一的消息类型推断与归一化。
pub struct MessageProfile {
    message_type: MessageType,
    message_type_label: String,
    processing_type: MessageProcessingType,
}

impl MessageProfile {
    pub fn ensure(message: &mut StorageMessage) -> Self {
        // 从 extra 中获取 message_type 标签，或从 content 推断
        let message_type_label = message
            .extra
            .get("message_type")
            .cloned()
            .or_else(|| {
                // 从 content 推断类型
                if let Some(content) = &message.content {
                    match content.content.as_ref() {
                        Some(flare_proto::common::message_content::Content::Text(_)) => Some("text".to_string()),
                        Some(flare_proto::common::message_content::Content::Image(_)) => Some("image".to_string()),
                        Some(flare_proto::common::message_content::Content::Video(_)) => Some("video".to_string()),
                        Some(flare_proto::common::message_content::Content::Audio(_)) => Some("audio".to_string()),
                        Some(flare_proto::common::message_content::Content::File(_)) => Some("file".to_string()),
                        Some(flare_proto::common::message_content::Content::Location(_)) => Some("location".to_string()),
                        Some(flare_proto::common::message_content::Content::Card(_)) => Some("card".to_string()),
                        Some(flare_proto::common::message_content::Content::Notification(_)) => Some("notification".to_string()),
                        Some(flare_proto::common::message_content::Content::Custom(custom)) => {
                            if custom.r#type.is_empty() {
                                Some("custom".to_string())
                            } else {
                                Some(custom.r#type.clone())
                            }
                        }
                        Some(flare_proto::common::message_content::Content::Forward(_)) => Some("forward".to_string()),
                        Some(flare_proto::common::message_content::Content::Typing(_)) => Some("typing".to_string()),
                        // 注意：Vote、Task、Schedule、Announcement 在新版 protobuf 中已移除
                        // Some(flare_proto::common::message_content::Content::Vote(_)) => Some("vote".to_string()),
                        // Some(flare_proto::common::message_content::Content::Task(_)) => Some("task".to_string()),
                        // Some(flare_proto::common::message_content::Content::Schedule(_)) => Some("schedule".to_string()),
                        // Some(flare_proto::common::message_content::Content::Announcement(_)) => Some("announcement".to_string()),
                        Some(flare_proto::common::message_content::Content::SystemEvent(_)) => Some("system_event".to_string()),
                        Some(flare_proto::common::message_content::Content::Quote(_)) => Some("quote".to_string()),
                        Some(flare_proto::common::message_content::Content::LinkCard(_)) => Some("link_card".to_string()),
                        None => None,
                    }
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "custom".to_string());

        // 根据标签推断 MessageType 枚举值（基于新的枚举定义，支持所有22种消息类型）
        let message_type = match message_type_label.as_str() {
            // 基础消息类型（9种）
            "text" | "text/plain" | "plain_text" => MessageType::Text,
            "image" => MessageType::Image,
            "video" => MessageType::Video,
            "audio" => MessageType::Audio,
            "file" => MessageType::File,
            "location" => MessageType::Location,
            "card" => MessageType::Card,
            "custom" | "json" | "sticker" | "command" | "event" | "system" => MessageType::Custom,
            "notification" => MessageType::Notification,
            
            // 功能消息类型（8种）
            "typing" => MessageType::Typing,
            "recall" => MessageType::Recall,
            "read" => MessageType::Read,
            "forward" => MessageType::Forward,
            // 注意：Vote、Task、Schedule、Announcement 在新版 protobuf 中已移除
            // "vote" => MessageType::Vote,
            // "task" => MessageType::Task,
            // "schedule" => MessageType::Schedule,
            // "announcement" => MessageType::Announcement,
            
            // 扩展消息类型（5种）
            "mini_program" | "miniprogram" => MessageType::MiniProgram,
            "link_card" | "linkcard" => MessageType::LinkCard,
            "quote" => MessageType::Quote,
            // 注意：Thread 在新版 protobuf 中已移除
            // "thread" => MessageType::Thread,
            "merge_forward" | "mergeforward" => MessageType::MergeForward,
            
            _ => MessageType::Unspecified,
        };

        // 设置 message_type 字段
        message.message_type = message_type as i32;

        // 将 message_type_label 保存到 extra
        message
            .extra
            .entry("message_type".into())
            .or_insert_with(|| message_type_label.clone());

        // 判断消息处理类型（Normal vs Notification）
        let processing_type = Self::determine_processing_type(&message_type_label, &message.extra);

        MessageProfile {
            message_type,
            message_type_label,
            processing_type,
        }
    }

    /// 判断消息处理类型
    /// 
    /// 规则：
    /// - 如果 extra 中有 `notification_only=true`，则为 Notification
    /// - 如果 message_type_label 为 "notification"，则为 Notification
    /// - 如果 message_type 为 Typing（正在输入），则为 Notification（不持久化）
    /// - 其他情况为 Normal
    fn determine_processing_type(
        message_type_label: &str,
        extra: &std::collections::HashMap<String, String>,
    ) -> MessageProcessingType {
        // 检查 extra 中的 notification_only 标志
        if let Some(flag) = extra.get("notification_only") {
            if flag == "true" || flag == "1" {
                return MessageProcessingType::Notification;
            }
        }

        // 检查 message_type_label
        if message_type_label == "notification" || message_type_label == "typing" {
            return MessageProcessingType::Notification;
        }

        // 默认为普通消息
        MessageProcessingType::Normal
    }

    pub fn message_type(&self) -> MessageType {
        self.message_type
    }

    pub fn message_type_label(&self) -> &str {
        &self.message_type_label
    }

    pub fn processing_type(&self) -> MessageProcessingType {
        self.processing_type
    }

    /// 判断是否需要持久化
    pub fn needs_persistence(&self) -> bool {
        self.processing_type == MessageProcessingType::Normal
    }

    /// 判断是否需要写入WAL
    pub fn needs_wal(&self) -> bool {
        self.processing_type == MessageProcessingType::Normal
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flare_proto::common::{Message, MessageContent, TextContent};

    fn message_with_extra(message_type_label: &str, message_type: i32) -> Message {
        let mut msg = Message::default();
        msg.message_type = message_type;
        msg.extra.insert("message_type".to_string(), message_type_label.to_string());
        msg.content = Some(MessageContent {
            content: Some(flare_proto::common::message_content::Content::Text(
                TextContent { text: "test".to_string(), mentions: vec![] }
            )),
            extensions: vec![],
        });
        msg
    }

    #[test]
    fn infer_from_extra_text() {
        let mut msg = message_with_extra("text", 0);
        let profile = MessageProfile::ensure(&mut msg);
        assert_eq!(profile.message_type(), MessageType::Text);
        assert_eq!(profile.message_type_label(), "text");
    }

    #[test]
    fn preserve_explicit_type() {
        let mut msg = message_with_extra("custom", MessageType::Custom as i32);
        let profile = MessageProfile::ensure(&mut msg);
        assert_eq!(profile.message_type(), MessageType::Custom);
        assert_eq!(profile.message_type_label(), "custom");
    }
}
