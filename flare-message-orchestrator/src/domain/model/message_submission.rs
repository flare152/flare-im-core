use anyhow::{Result, anyhow};
use chrono::Utc;
use flare_im_core::utils::{TimelineMetadata, current_millis, datetime_to_timestamp, timestamp_to_millis, embed_timeline_in_extra};
use flare_proto::storage::StoreMessageRequest;
use uuid::Uuid;

use crate::domain::model::message_kind::MessageProfile;

#[derive(Clone, Debug)]
pub struct MessageDefaults {
    pub default_business_type: String,
    pub default_session_type: String,
    pub default_sender_type: String,
    pub default_tenant_id: Option<String>,
}

#[derive(Clone)]
pub struct MessageSubmission {
    pub kafka_payload: StoreMessageRequest,
    pub message: flare_proto::common::Message,
    pub message_id: String,
    pub timeline: TimelineMetadata,
}

impl MessageSubmission {
    pub fn prepare(mut request: StoreMessageRequest, defaults: &MessageDefaults) -> Result<Self> {
        if request.session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        let mut message = request
            .message
            .take()
            .ok_or_else(|| anyhow!("message payload is required"))?;

        if message.session_id.is_empty() {
            message.session_id = request.session_id.clone();
        }

        if message.id.is_empty() {
            message.id = Uuid::new_v4().to_string();
        }

        if message.sender_id.is_empty() {
            return Err(anyhow!("sender_id is required"));
        }

        // 如果 source 未设置，使用默认值（从 sender_type 迁移到 source 枚举）
        if message.source == 0 { // MessageSource::Unspecified = 0
            // 根据 default_sender_type 设置 source
            message.source = match defaults.default_sender_type.as_str() {
                "user" => 1, // MessageSource::User
                "system" => 2, // MessageSource::System
                "bot" => 3, // MessageSource::Bot
                "admin" => 4, // MessageSource::Admin
                _ => 1, // 默认为 User
            };
        }

        if message.business_type.is_empty() {
            message.business_type = defaults.default_business_type.clone();
        }

        // session_type 是 i32 枚举，0 表示未设置（Unspecified）
        if message.session_type == 0 {
            // 将字符串类型的默认 session_type 转换为 i32 枚举
            message.session_type = match defaults.default_session_type.as_str() {
                "single" => 1,   // SessionType::Single
                "group" => 2,    // SessionType::Group
                "channel" => 3,  // SessionType::Channel
                _ => 1,           // 默认为 Single
            };
        }

        // 如果 status 未设置，使用默认值（从 string 迁移到 MessageStatus 枚举）
        if message.status == 0 { // MessageStatus::Unspecified = 0
            message.status = 1; // MessageStatus::Created = 1
        }

        let profile = MessageProfile::ensure(&mut message);
        if message.extra.get("message_type").is_none() {
            message.extra.insert(
                "message_type".into(),
                profile.message_type_label().to_string(),
            );
        }

        if message.timestamp.is_none() {
            message.timestamp = Some(datetime_to_timestamp(Utc::now()));
        }

        let ingestion_ts = current_millis();
        let emit_ts = message
            .timestamp
            .as_ref()
            .and_then(|ts| timestamp_to_millis(ts));

        let shard_key = message
            .extra
            .get("shard_key")
            .cloned()
            .unwrap_or_else(|| request.session_id.clone());
        message
            .extra
            .entry("shard_key".to_string())
            .or_insert(shard_key.clone());

        let tenant_id = message
            .extra
            .get("tenant_id")
            .cloned()
            .or_else(|| defaults.default_tenant_id.clone());
        if let Some(ref tenant) = tenant_id {
            message
                .extra
                .entry("tenant_id".to_string())
                .or_insert(tenant.clone());
        }

        let timeline = TimelineMetadata {
            emit_ts,
            ingestion_ts,
            ..TimelineMetadata::default()
        };

        // 将时间线信息嵌入到消息的 extra 中
        embed_timeline_in_extra(&mut message, &timeline);

        // 清理字符串字段，确保所有字段都是有效的 UTF-8
        // 注意：新版 Message 结构已移除 sender_platform_id、sender_nickname、
        // sender_avatar_url、group_id、receiver_id 等字段，这些信息现在通过
        // attributes 或 extra 字段存储
        message.client_msg_id = String::from_utf8_lossy(
            message.client_msg_id.as_bytes()
        ).to_string();
        
        // 清理消息内容中的 text 字段，确保它是有效的 UTF-8
        // 这可以避免 Protobuf 序列化/反序列化时的编码问题
        if let Some(ref mut content) = message.content {
            if let Some(flare_proto::common::message_content::Content::Text(ref mut text_content)) = content.content {
                // 清理 text 字段：
                // 1. 确保 UTF-8 编码
                // 2. 移除控制字符（如 \x08 退格字符、\x00 空字符等）
                // 3. 保留可打印字符和空白字符（空格、换行、制表符等）
                let cleaned: String = text_content.text
                    .chars()
                    .filter(|c| {
                        // 保留空白字符（空格、换行、制表符等）
                        if c.is_whitespace() {
                            true
                        } else {
                            // 过滤掉所有控制字符（包括 \x08 退格字符）
                            !c.is_control()
                        }
                    })
                    .collect();
                
                // 去除首尾空白
                text_content.text = cleaned.trim().to_string();
            }
        }

        let message_id = message.id.clone();

        let kafka_payload = StoreMessageRequest {
            session_id: request.session_id,
            message: Some(message.clone()),
            sync: request.sync,
            context: request.context,
            tenant: request.tenant,
            tags: request.tags,
        };

        Ok(Self {
            kafka_payload,
            message,
            message_id,
            timeline,
        })
    }
}
