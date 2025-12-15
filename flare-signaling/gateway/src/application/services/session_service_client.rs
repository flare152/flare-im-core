//! 会话服务客户端
//!
//! 为消息应用服务提供对会话服务的访问能力

use std::sync::Arc;
use flare_core::common::error::{Result, FlareError};
use flare_session::application::commands::UpdateCursorCommand;
use flare_session::application::SessionCommandHandler;

/// 会话服务客户端
///
/// 职责：
/// - 提供对会话服务的访问接口
/// - 封装会话服务调用的细节
pub struct SessionServiceClient {
    session_command_handler: Arc<SessionCommandHandler>,
}

impl SessionServiceClient {
    pub fn new(session_command_handler: Arc<SessionCommandHandler>) -> Self {
        Self {
            session_command_handler,
        }
    }

    /// 更新会话游标
    ///
    /// 当收到客户端ACK时，更新用户的会话游标位置
    pub async fn update_session_cursor(
        &self,
        user_id: &str,
        session_id: &str,
        message_ts: i64,
    ) -> Result<()> {
        let command = UpdateCursorCommand {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            message_ts,
        };

        self.session_command_handler
            .handle_update_cursor(command)
            .await
            .map_err(|e| FlareError::general_error(format!("Failed to update session cursor: {}", e)))
    }
}