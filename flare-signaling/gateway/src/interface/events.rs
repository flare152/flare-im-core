//! 事件处理器模块
//!
//! 处理各种系统事件和命令

use std::sync::Arc;

use async_trait::async_trait;
use flare_core::common::error::Result;
use flare_core::common::protocol::{Frame, MessageCommand, NotificationCommand};
use flare_core::server::events::handler::ServerEventHandler;
use tracing::{debug, error, info, warn};

use crate::interface::connection::LongConnectionHandler;

/// 网关事件处理器
pub struct GatewayEventHandler {
    connection_handler: Arc<LongConnectionHandler>,
}

impl GatewayEventHandler {
    pub fn new(connection_handler: Arc<LongConnectionHandler>) -> Self {
        Self { connection_handler }
    }
}

#[async_trait]
impl ServerEventHandler for GatewayEventHandler {
    /// 处理消息命令
    async fn handle_message_command(
        &self,
        command: &MessageCommand,
        connection_id: &str,
    ) -> Result<Option<Frame>> {
        debug!(
            "[EventHandler] 📨 收到消息命令: connection_id={}, message_type={}, message_id={}, payload_len={}",
            connection_id,
            command.r#type,
            command.message_id,
            command.payload.len()
        );

        if let Err(err) = self.connection_handler.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session via event");
        }

        Ok(None)
    }

    /// 处理通知命令
    async fn handle_notification_command(
        &self,
        command: &NotificationCommand,
        connection_id: &str,
    ) -> Result<Option<Frame>> {
        debug!(
            "[EventHandler] 🔔 收到通知命令: connection_id={}, notification_type={}, title={}, content_len={}",
            connection_id,
            command.r#type,
            command.title,
            command.content.len()
        );

        Ok(None)
    }

    /// 处理 CONNECT 系统命令
    async fn handle_connect(&self, _frame: &Frame, connection_id: &str) -> Result<Option<Frame>> {
        debug!(
            "[EventHandler] 🔌 收到 CONNECT 命令: connection_id={}",
            connection_id
        );
        Ok(None)
    }

    /// 处理 PING 系统命令
    async fn handle_ping(&self, _frame: &Frame, connection_id: &str) -> Result<Option<Frame>> {
        debug!(
            "[EventHandler] 💓 收到 PING: connection_id={}",
            connection_id
        );
        if let Err(err) = self.connection_handler.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session on ping");
        }
        Ok(None)
    }

    /// 处理 PONG 系统命令
    async fn handle_pong(&self, _frame: &Frame, connection_id: &str) -> Result<Option<Frame>> {
        debug!(
            "[EventHandler] 💓 收到 PONG: connection_id={}",
            connection_id
        );
        if let Err(err) = self.connection_handler.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session on pong");
        }
        Ok(None)
    }

    /// 处理连接断开事件
    async fn on_disconnect(&self, connection_id: &str, reason: Option<&str>) -> Result<()> {
        if let Some(reason) = reason {
            info!(
                "[EventHandler] 🔌 连接断开: connection_id={}, reason={}",
                connection_id, reason
            );
        } else {
            info!(
                "[EventHandler] 🔌 连接断开: connection_id={}",
                connection_id
            );
        }

        if let Err(err) = self.connection_handler.refresh_session(connection_id).await {
            warn!(?err, %connection_id, "failed to refresh session on disconnect");
        }

        Ok(())
    }

    /// 处理连接错误事件
    async fn on_error(&self, connection_id: &str, error: &str) -> Result<()> {
        tracing::error!(
            "[EventHandler] ❌ 连接错误: connection_id={}, error={}",
            connection_id,
            error
        );
        Ok(())
    }
}
