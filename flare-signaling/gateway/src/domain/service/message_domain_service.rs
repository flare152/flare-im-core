//! 消息领域服务
//!
//! 封装消息处理的核心业务逻辑

use flare_core::common::error::{FlareError, Result};
use flare_core::common::protocol::MessageCommand;
use std::sync::Arc;
use tracing::debug;

/// 消息领域服务
///
/// 职责：
/// - 封装消息解析逻辑
/// - 封装消息验证逻辑
/// - 提供消息处理的核心业务规则
pub struct MessageDomainService;

impl MessageDomainService {
    pub fn new() -> Self {
        Self
    }

    /// 从 MessageCommand 中提取 conversation_id
    ///
    /// # 业务规则
    /// 1. **优先从 metadata 获取**：SDK 已将 conversation_id 放入 MessageCommand.metadata
    ///    - 性能更好：无需解析整个 payload
    ///    - 更可靠：不依赖 payload 格式
    /// 2. **验证规则**：
    ///    - conversation_id 不能为空
    ///    - metadata 中的 conversation_id 必须是有效的 UTF-8 字符串
    ///
    /// # 为什么不在 metadata 中？
    /// - SDK 端（message_sender.rs）已设置：`metadata.insert("conversation_id", ...)`
    /// - Gateway 端应优先使用 metadata，避免不必要的 payload 解析
    pub fn extract_conversation_id(&self, msg_cmd: &MessageCommand) -> Result<String> {
        // 优先从 metadata 获取（推荐方式）
        // SDK 端（message_sender.rs）已设置：metadata.insert("conversation_id", ...)
        if let Some(conversation_id_bytes) = msg_cmd.metadata.get("conversation_id") {
            match String::from_utf8(conversation_id_bytes.clone()) {
                Ok(conversation_id) if !conversation_id.is_empty() => {
                    debug!(
                        conversation_id = %conversation_id,
                        source = "metadata",
                        "Extracted conversation_id from metadata"
                    );
                    return Ok(conversation_id);
                }
                Ok(_) => {
                    return Err(FlareError::message_format_error(
                        "conversation_id in metadata is empty",
                    ));
                }
                Err(e) => {
                    return Err(FlareError::deserialization_error(format!(
                        "Failed to decode conversation_id from metadata: {}",
                        e
                    )));
                }
            }
        }

        // conversation_id 未在 metadata 中找到，返回错误
        Err(FlareError::message_format_error(
            "conversation_id is required but not found in metadata. Please ensure SDK sets conversation_id in MessageCommand.metadata",
        ))
    }

    /// 验证消息格式
    ///
    /// 验证消息是否符合业务规则
    pub fn validate_message(&self, msg_cmd: &MessageCommand) -> Result<()> {
        // 验证消息ID不为空
        if msg_cmd.message_id.is_empty() {
            return Err(FlareError::message_format_error(
                "message_id is required but empty",
            ));
        }

        // 验证 payload 不为空
        if msg_cmd.payload.is_empty() {
            return Err(FlareError::message_format_error(
                "message payload is required but empty",
            ));
        }

        Ok(())
    }
}

impl Default for MessageDomainService {
    fn default() -> Self {
        Self::new()
    }
}

