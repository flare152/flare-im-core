//! 会话服务客户端
//!
//! 为消息应用服务提供对会话服务的访问能力

use flare_core::common::error::{FlareError, Result};
use flare_conversation::application::ConversationCommandHandler;
use flare_conversation::application::commands::UpdateCursorCommand;
use std::sync::Arc;

/// 会话服务客户端
///
/// 职责：
/// - 提供对会话服务的访问接口
/// - 封装会话服务调用的细节
pub struct ConversationServiceClient {
    conversation_command_handler: Arc<ConversationCommandHandler>,
}

impl ConversationServiceClient {
    pub fn new(conversation_command_handler: Arc<ConversationCommandHandler>) -> Self {
        Self {
            conversation_command_handler,
        }
    }

    /// 更新会话游标
    ///
    /// 当收到客户端ACK时，更新用户的会话游标位置
    pub async fn update_session_cursor(
        &self,
        user_id: &str,
        conversation_id: &str,
        message_ts: i64,
    ) -> Result<()> {
        let command = UpdateCursorCommand {
            user_id: user_id.to_string(),
            conversation_id: conversation_id.to_string(),
            message_ts,
        };

        self.conversation_command_handler
            .handle_update_cursor(command)
            .await
            .map_err(|e| {
                FlareError::general_error(format!("Failed to update session cursor: {}", e))
            })
    }
}
