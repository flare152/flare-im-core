//! 自定义命令处理模块
//!
//! 处理客户端发送的自定义命令（Custom Command）

use flare_core::common::error::{FlareError as CoreFlareError, Result as CoreResult};
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::{Frame, Reliability};
use prost::Message as ProstMessage;
use tracing::debug;

use super::connection::LongConnectionHandler;

impl LongConnectionHandler {
    /// 处理消息帧的内部实现（协议适配层）
    /// 
    /// **职责**：只处理自定义命令（Custom Command）
    /// - 消息命令（Message）和通知命令（Notification）由 `ServerEventHandler` 处理
    /// - 这样可以充分利用 `flare-core` 的 `DefaultServerMessageObserver` 的自动路由功能
    pub(crate) async fn handle_frame_impl(
        &self,
        frame: &Frame,
        connection_id: &str,
    ) -> CoreResult<Option<Frame>> {
        // 只处理自定义命令（Custom Command）
        if let Some(cmd) = &frame.command {
            if let Some(CommandType::Custom(custom_cmd)) = &cmd.r#type {
                let request_id = frame
                    .metadata
                    .get("request_id")
                    .and_then(|v| String::from_utf8(v.clone()).ok())
                    .unwrap_or_else(|| frame.message_id.clone());

                match custom_cmd.name.as_str() {
                    "ConversationBootstrap" => {
                        return self.handle_conversation_bootstrap(custom_cmd, request_id).await;
                    }
                    "SyncMessages" => {
                        return self.handle_sync_messages(custom_cmd, request_id).await;
                    }
                    "ListSessions" => {
                        return self.handle_list_sessions(custom_cmd, request_id).await;
                    }
                    _ => {
                        debug!(
                            connection_id = %connection_id,
                            command_name = %custom_cmd.name,
                            "Unhandled custom command"
                        );
                    }
                }
            } else {
                // 其他命令类型（Message、Notification）由 ServerEventHandler 处理
                // 这里不应该到达，因为 DefaultServerMessageObserver 会先路由到 ServerEventHandler
                debug!(
                    connection_id = %connection_id,
                    message_id = %frame.message_id,
                    "Frame command type should be handled by ServerEventHandler"
                );
            }
        }
        
        // 没有匹配的自定义命令，返回 None
        Ok(None)
    }

    /// 处理 ConversationBootstrap 自定义命令
    async fn handle_conversation_bootstrap(
        &self,
        custom_cmd: &flare_core::common::protocol::CustomCommand,
        request_id: String,
    ) -> CoreResult<Option<Frame>> {
        use flare_proto::conversation::{
            ConversationBootstrapRequest, ConversationBootstrapResponse,
        };
        let req =
            ConversationBootstrapRequest::decode(&custom_cmd.data[..]).map_err(|e| {
                CoreFlareError::deserialization_error(format!(
                    "decode ConversationBootstrapRequest: {}",
                    e
                ))
            })?;
        let mut client = self.ensure_conversation_client().await?;
        let resp = client
            .conversation_bootstrap(req)
            .await
            .map_err(|status| CoreFlareError::system(status.to_string()))?
            .into_inner();
        let mut buf = Vec::new();
        ConversationBootstrapResponse::encode(&resp, &mut buf).map_err(|e| {
            CoreFlareError::serialization_error(format!(
                "encode ConversationBootstrapResponse: {}",
                e
            ))
        })?;
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        let response_frame =
            flare_core::common::protocol::builder::FrameBuilder::new()
                .with_command(
                    flare_core::common::protocol::flare::core::commands::Command {
                        r#type: Some(CommandType::Custom(
                            flare_core::common::protocol::CustomCommand {
                                name: "ConversationBootstrap".to_string(),
                                data: buf,
                                metadata,
                            },
                        )),
                    },
                )
                .with_message_id(request_id)
                .with_reliability(Reliability::AtLeastOnce)
                .build();
        Ok(Some(response_frame))
    }

    /// 处理 SyncMessages 自定义命令
    async fn handle_sync_messages(
        &self,
        custom_cmd: &flare_core::common::protocol::CustomCommand,
        request_id: String,
    ) -> CoreResult<Option<Frame>> {
        use flare_proto::conversation::{SyncMessagesRequest, SyncMessagesResponse};
        use prost::Message as _;
        let req =
            SyncMessagesRequest::decode(&custom_cmd.data[..]).map_err(|e| {
                CoreFlareError::deserialization_error(format!(
                    "decode SyncMessagesRequest: {}",
                    e
                ))
            })?;
        let mut client = self.ensure_conversation_client().await?;
        let resp = client
            .sync_messages(req)
            .await
            .map_err(|status| CoreFlareError::system(status.to_string()))?
            .into_inner();
        let mut buf = Vec::new();
        SyncMessagesResponse::encode(&resp, &mut buf).map_err(|e| {
            CoreFlareError::serialization_error(format!(
                "encode SyncMessagesResponse: {}",
                e
            ))
        })?;
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        let response_frame =
            flare_core::common::protocol::builder::FrameBuilder::new()
                .with_command(
                    flare_core::common::protocol::flare::core::commands::Command {
                        r#type: Some(CommandType::Custom(
                            flare_core::common::protocol::CustomCommand {
                                name: "SyncMessages".to_string(),
                                data: buf,
                                metadata,
                            },
                        )),
                    },
                )
                .with_message_id(request_id)
                .with_reliability(Reliability::AtLeastOnce)
                .build();
        Ok(Some(response_frame))
    }

    /// 处理 ListSessions 自定义命令
    async fn handle_list_sessions(
        &self,
        custom_cmd: &flare_core::common::protocol::CustomCommand,
        request_id: String,
    ) -> CoreResult<Option<Frame>> {
        use flare_proto::conversation::{ListConversationsRequest, ListConversationsResponse};
        let req =
            ListConversationsRequest::decode(&custom_cmd.data[..]).map_err(|e| {
                CoreFlareError::deserialization_error(format!(
                    "decode ListConversationsRequest: {}",
                    e
                ))
            })?;
        let mut client = self.ensure_conversation_client().await?;
        let resp = client
            .list_conversations(req)
            .await
            .map_err(|status| CoreFlareError::system(status.to_string()))?
            .into_inner();
        let mut buf = Vec::new();
        if let Err(e) = ListConversationsResponse::encode(&resp, &mut buf) {
            return Err(CoreFlareError::serialization_error(format!(
                "encode ListConversationsResponse: {}",
                e
            )));
        }
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("request_id".to_string(), request_id.as_bytes().to_vec());
        let response_frame =
            flare_core::common::protocol::builder::FrameBuilder::new()
                .with_command(
                    flare_core::common::protocol::flare::core::commands::Command {
                        r#type: Some(CommandType::Custom(
                            flare_core::common::protocol::CustomCommand {
                                name: "ListSessions".to_string(),
                                data: buf,
                                metadata,
                            },
                        )),
                    },
                )
                .with_message_id(request_id)
                .with_reliability(Reliability::AtLeastOnce)
                .build();
        Ok(Some(response_frame))
    }
}
