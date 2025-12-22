//! 消息推送模块
//!
//! 提供向客户端推送消息的功能

use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::{MessageCommand, Reliability, frame_with_message_command, generate_message_id};
use tracing::{debug, info};

use super::connection::LongConnectionHandler;

impl LongConnectionHandler {
    /// 推送消息到客户端
    pub async fn push_message_to_user(&self, user_id: &str, message: Vec<u8>) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system(
                    "ServerHandle not initialized".to_string(),
                ));
            }
        };

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
            .map_err(|e| CoreFlareError::system(format!("Failed to send message: {}", e)))?;

        info!(
            user_id = %user_id,
            "Message pushed to user"
        );
        Ok(())
    }

    /// 推送消息到指定连接
    pub async fn push_message_to_connection(
        &self,
        connection_id: &str,
        message: Vec<u8>,
    ) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system(
                    "ServerHandle not initialized".to_string(),
                ));
            }
        };

        let cmd = MessageCommand {
            r#type: 0,
            message_id: generate_message_id(),
            payload: message,
            metadata: Default::default(),
            seq: 0,
        };

        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

        handle
            .send_to(connection_id, &frame)
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to send message: {}", e)))?;

        debug!(
            connection_id = %connection_id,
            "Message pushed to connection"
        );
        Ok(())
    }

    /// 推送数据包到指定连接
    pub async fn push_packet_to_connection(
        &self,
        connection_id: &str,
        packet: &flare_proto::common::ServerPacket,
    ) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system(
                    "ServerHandle not initialized".to_string(),
                ));
            }
        };

        // 将 ServerPacket 序列化为字节
        use prost::Message as _;
        let mut packet_data = Vec::new();
        packet.encode(&mut packet_data).map_err(|e| {
            CoreFlareError::serialization_error(format!("Failed to encode ServerPacket: {}", e))
        })?;

        // 创建推送命令
        let cmd = MessageCommand {
            r#type: 0, // 普通消息类型
            message_id: generate_message_id(),
            payload: packet_data,
            metadata: Default::default(),
            seq: 0,
        };

        let message_id = cmd.message_id.clone();
        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

        handle
            .send_to(connection_id, &frame)
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to send packet: {}", e)))?;

        debug!(
            connection_id = %connection_id,
            message_id = %message_id,
            "ServerPacket pushed to connection"
        );
        Ok(())
    }

    /// 推送数据包到指定用户的所有连接
    pub async fn push_packet_to_user(
        &self,
        user_id: &str,
        packet: &flare_proto::common::ServerPacket,
    ) -> CoreResult<()> {
        let handle_guard = self.server_handle.lock().await;
        let handle = match handle_guard.as_ref() {
            Some(handle) => handle,
            None => {
                return Err(CoreFlareError::system(
                    "ServerHandle not initialized".to_string(),
                ));
            }
        };

        // 将 ServerPacket 序列化为字节
        use prost::Message as _;
        let mut packet_data = Vec::new();
        packet.encode(&mut packet_data).map_err(|e| {
            CoreFlareError::serialization_error(format!("Failed to encode ServerPacket: {}", e))
        })?;

        // 创建推送命令
        let cmd = MessageCommand {
            r#type: 0, // 普通消息类型
            message_id: generate_message_id(),
            payload: packet_data,
            metadata: Default::default(),
            seq: 0,
        };

        let message_id = cmd.message_id.clone();
        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);

        handle
            .send_to_user(user_id, &frame)
            .await
            .map_err(|e| CoreFlareError::system(format!("Failed to send packet: {}", e)))?;

        info!(
            user_id = %user_id,
            message_id = %message_id,
            "ServerPacket pushed to user"
        );
        Ok(())
    }
}
