//! 连接处理器模块
//!
//! 处理客户端长连接的消息接收和推送

use std::sync::Arc;

use async_trait::async_trait;
use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::{
    Frame, MessageCommand, Reliability, frame_with_message_command, generate_message_id,
};
use flare_core::server::handle::ServerHandle;
use flare_core::server::{ConnectionHandler, ConnectionManagerTrait};
use flare_server_core::error::Result;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::domain::SessionStore;

/// 长连接处理器
///
/// 处理客户端长连接的消息接收和推送
pub struct LongConnectionHandler {
    session_store: Arc<dyn SessionStore>,
    server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
    manager_trait: Arc<Mutex<Option<Arc<dyn ConnectionManagerTrait>>>>,
}

impl LongConnectionHandler {
    pub fn new(session_store: Arc<dyn SessionStore>) -> Self {
        Self {
            session_store,
            server_handle: Arc::new(Mutex::new(None)),
            manager_trait: Arc::new(Mutex::new(None)),
        }
    }

    /// 设置 ServerHandle
    pub async fn set_server_handle(&self, handle: Arc<dyn ServerHandle>) {
        *self.server_handle.lock().await = Some(handle);
    }

    /// 设置 ConnectionManagerTrait
    pub async fn set_connection_manager(&self, manager: Arc<dyn ConnectionManagerTrait>) {
        *self.manager_trait.lock().await = Some(manager);
    }

    /// 获取用户ID（从连接信息中提取）
    pub async fn user_id_for_connection(&self, connection_id: &str) -> Option<String> {
        if let Some(ref manager) = *self.manager_trait.lock().await {
            if let Some((_, conn_info)) = manager.get_connection(connection_id).await {
                return conn_info.user_id.clone();
            }
        }
        None
    }

    /// 主动断开指定连接
    pub async fn disconnect_connection(&self, connection_id: &str) {
        if let Some(handle) = self.server_handle.lock().await.clone() {
            if let Err(err) = handle.disconnect(connection_id).await {
                warn!(?err, %connection_id, "failed to disconnect connection");
            }
        } else {
            warn!(%connection_id, "disconnect requested but server handle not ready");
        }
    }

    /// 刷新连接对应会话的心跳
    pub async fn refresh_session(&self, connection_id: &str) -> Result<()> {
        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            let sessions = self.session_store.find_by_user(&user_id).await?;
            for session in sessions {
                if session.connection_id.as_deref() == Some(connection_id) {
                    let _ = self.session_store.touch(&session.session_id).await?;
                }
            }
        }
        Ok(())
    }

    /// 推送消息到客户端
    pub async fn push_message_to_user(
        &self,
        user_id: &str,
        message: Vec<u8>,
    ) -> std::result::Result<(), Box<dyn std::error::Error>> {
        if let Some(ref handle) = *self.server_handle.lock().await {
            let cmd = MessageCommand {
                r#type: 0,
                message_id: generate_message_id(),
                payload: message,
                metadata: Default::default(),
                seq: 0,
            };

            let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

            handle
                .send_to_user(user_id, &frame)
                .await
                .map_err(|e| format!("Failed to send message: {}", e))?;

            info!("Pushed message to user {}", user_id);
        } else {
            return Err("ServerHandle not initialized".into());
        }

        Ok(())
    }
}

#[async_trait]
impl ConnectionHandler for LongConnectionHandler {
    async fn handle_frame(&self, frame: &Frame, connection_id: &str) -> CoreResult<Option<Frame>> {
        debug!(
            "Received frame from connection {}: {:?}",
            connection_id, frame
        );

        if let Some(cmd) = &frame.command {
            if let Some(CommandType::Message(msg_cmd)) = &cmd.r#type {
                let message_type = msg_cmd.r#type;

                if message_type == 0 {
                    let user_id = self
                        .user_id_for_connection(connection_id)
                        .await
                        .unwrap_or_else(|| "unknown".to_string());

                    info!(
                        "📨 收到消息: user_id={}, connection_id={}, message_len={}",
                        user_id,
                        connection_id,
                        msg_cmd.payload.len()
                    );

                    if let Err(err) = self.refresh_session(connection_id).await {
                        warn!(?err, %connection_id, "failed to refresh session heartbeat");
                    }
                }
            }
        }

        Ok(None)
    }

    async fn on_connect(&self, connection_id: &str) -> CoreResult<()> {
        info!("✅ 新连接: {}", connection_id);

        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            let sessions = self
                .session_store
                .find_by_user(&user_id)
                .await
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            for session in sessions {
                self.session_store
                    .update_connection(&session.session_id, Some(connection_id.to_string()))
                    .await
                    .map_err(|err| CoreFlareError::system(err.to_string()))?;
            }

            info!(
                "📝 用户已连接: user_id={}, connection_id={}",
                user_id, connection_id
            );
        }

        Ok(())
    }

    async fn on_disconnect(&self, connection_id: &str) -> CoreResult<()> {
        info!("❌ 连接断开: {}", connection_id);

        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            let sessions = self
                .session_store
                .find_by_user(&user_id)
                .await
                .map_err(|err| CoreFlareError::system(err.to_string()))?;
            for session in sessions {
                self.session_store
                    .update_connection(&session.session_id, None)
                    .await
                    .map_err(|err| CoreFlareError::system(err.to_string()))?;
            }
            info!(
                "📝 用户已断开: user_id={}, connection_id={}",
                user_id, connection_id
            );
        }

        Ok(())
    }
}
