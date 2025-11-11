use std::collections::HashMap;
use std::sync::Arc;

use flare_core::common::compression::CompressionAlgorithm;
use flare_core::common::config_types::TransportProtocol;
use flare_core::common::device::DeviceConflictStrategyBuilder;
use flare_core::common::error::Result;
use flare_core::common::protocol::flare::core::commands::command::Type as CommandType;
use flare_core::common::protocol::flare::core::commands::system_command::SerializationFormat;
use flare_core::common::protocol::{
    Frame, MessageCommand, NotificationCommand, Reliability, frame_with_message_command,
    generate_message_id,
};
use flare_core::server::connection::{ConnectionManager, ConnectionManagerTrait};
use flare_core::server::device::DeviceManager;
use flare_core::server::events::handler::ServerEventHandler;
use flare_core::server::handle::{DefaultServerHandle, ServerHandle};
use flare_core::server::{ConnectionHandler, ObserverServerBuilder};
use flare_im_core::config::ServiceRuntimeConfig;
use flare_im_core::{load_config, register_service};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

/// 演示如何基于 flare-im-core 提供的配置/注册能力构建一个可协商序列化格式并支持设备互斥的聊天室服务。
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    // 读取 flare-im-core 的统一配置（默认从 ./config 目录加载，可通过 FLARE_CONFIG_DIR 覆盖）
    let app_config = load_config(Some("config"));
    let runtime_config = app_config.compose_service_config(
        &ServiceRuntimeConfig {
            service_name: Some("flare.negotiation.chat".to_string()),
            ..Default::default()
        },
        "flare.negotiation.chat",
    );

    // 注册到统一的服务注册中心（如未配置将自动跳过）
    if let Err(err) = register_service(&runtime_config, "negotiation-chat-service").await {
        tracing::warn!(error = ?err, "service registration skipped");
    }

    let listen_addr = format!(
        "{}:{}",
        runtime_config.server.address, runtime_config.server.port
    );
    let quic_addr = format!(
        "{}:{}",
        runtime_config.server.address,
        runtime_config.server.port + 1,
    );

    info!(%listen_addr, %quic_addr, "🚀 启动协商聊天室服务器");

    // 设备互斥策略：同一用户同一平台同时仅允许一个设备在线
    let device_manager = Arc::new(DeviceManager::new(
        DeviceConflictStrategyBuilder::new()
            .platform_exclusive()
            .build(),
    ));

    let connection_manager = Arc::new(ConnectionManager::new());

    let handler = Arc::new(NegotiationChatRoomHandler {
        usernames: Arc::new(Mutex::new(HashMap::new())),
        server_handle: Arc::new(Mutex::new(None)),
        connection_manager: Arc::new(Mutex::new(None)),
        listen_ws: listen_addr.clone(),
        listen_quic: quic_addr.clone(),
    });

    let event_handler = Arc::new(DebugEventHandler);

    let mut server = ObserverServerBuilder::new(listen_addr.clone())
        .with_handler(handler.clone() as Arc<dyn ConnectionHandler>)
        .with_connection_manager(connection_manager.clone())
        .with_device_manager(device_manager)
        .with_event_handler(event_handler)
        .with_default_format(SerializationFormat::Protobuf)
        .with_default_compression(CompressionAlgorithm::None)
        .with_protocols(vec![TransportProtocol::WebSocket, TransportProtocol::QUIC])
        .with_protocol_address(TransportProtocol::WebSocket, listen_addr)
        .with_protocol_address(TransportProtocol::QUIC, quic_addr)
        .build()?;

    let (server_handle, manager_trait) = if let Some(components) =
        server.get_server_handle_components()
    {
        let handle: Arc<dyn ServerHandle> = Arc::new(DefaultServerHandle::new(components.clone()));
        (handle, components)
    } else {
        return Err("无法获取连接管理器".into());
    };
    handler.set_server_handle(server_handle).await;
    handler.set_connection_manager(manager_trait).await;

    server.start().await?;
    info!("✅ 协商聊天室服务器已启动");
    info!("   WebSocket: ws://{}", handler.listen_ws);
    info!("   QUIC: quic://{}", handler.listen_quic);

    tokio::signal::ctrl_c().await?;
    info!("⌛ 停止服务器...");
    server.stop().await?;
    info!("服务器已停止");

    Ok(())
}

/// 用于打印事件的调试处理器。
struct DebugEventHandler;

#[async_trait::async_trait]
impl ServerEventHandler for DebugEventHandler {
    async fn handle_message_command(
        &self,
        command: &MessageCommand,
        connection_id: &str,
    ) -> Result<Option<Frame>> {
        let payload_preview = String::from_utf8_lossy(&command.payload);
        info!(%connection_id, message_id = %command.message_id, "💬 收到消息: {payload_preview}");
        Ok(None)
    }

    async fn handle_notification_command(
        &self,
        command: &NotificationCommand,
        connection_id: &str,
    ) -> Result<Option<Frame>> {
        let preview = String::from_utf8_lossy(&command.content);
        info!(%connection_id, title = %command.title, "🔔 收到通知: {preview}");
        Ok(None)
    }
}

struct NegotiationChatRoomHandler {
    usernames: Arc<Mutex<HashMap<String, String>>>,
    server_handle: Arc<Mutex<Option<Arc<dyn ServerHandle>>>>,
    connection_manager: Arc<Mutex<Option<Arc<dyn ConnectionManagerTrait>>>>,
    listen_ws: String,
    listen_quic: String,
}

impl NegotiationChatRoomHandler {
    async fn set_server_handle(&self, handle: Arc<dyn ServerHandle>) {
        *self.server_handle.lock().await = Some(handle);
    }

    async fn set_connection_manager(&self, manager: Arc<dyn ConnectionManagerTrait>) {
        *self.connection_manager.lock().await = Some(manager);
    }
}

#[async_trait::async_trait]
impl ConnectionHandler for NegotiationChatRoomHandler {
    async fn handle_frame(&self, frame: &Frame, connection_id: &str) -> Result<Option<Frame>> {
        if let Some(cmd) = &frame.command {
            if let Some(CommandType::Message(message)) = &cmd.r#type {
                if message.r#type == 0 {
                    let username = self
                        .usernames
                        .lock()
                        .await
                        .get(connection_id)
                        .cloned()
                        .unwrap_or_else(|| "匿名".to_string());

                    let text = String::from_utf8_lossy(&message.payload);
                    info!(%connection_id, %username, "💬 {text}");

                    let broadcast_cmd = MessageCommand {
                        r#type: 0,
                        message_id: generate_message_id(),
                        payload: format!("[{username}]: {text}").into_bytes(),
                        metadata: HashMap::new(),
                        seq: 0,
                    };
                    let frame = frame_with_message_command(broadcast_cmd, Reliability::AtLeastOnce);

                    if let Some(handle) = self.server_handle.lock().await.as_ref() {
                        if let Err(err) = handle.broadcast_except(&frame, connection_id).await {
                            error!(?err, "广播消息失败");
                        }
                    }
                }
            }
        }

        Ok(None)
    }

    async fn on_connect(&self, connection_id: &str) -> Result<()> {
        debug!(%connection_id, "新连接加入");

        let username = if let Some(manager) = self.connection_manager.lock().await.as_ref() {
            match manager.get_connection(connection_id).await {
                Some((_, info)) => info.user_id.clone().unwrap_or_else(|| {
                    format!("用户_{}", &connection_id[..connection_id.len().min(6)])
                }),
                None => format!("用户_{}", &connection_id[..connection_id.len().min(6)]),
            }
        } else {
            format!("用户_{}", &connection_id[..connection_id.len().min(6)])
        };

        self.usernames
            .lock()
            .await
            .insert(connection_id.to_string(), username.clone());

        if let Some(handle) = self.server_handle.lock().await.as_ref() {
            let welcome = MessageCommand {
                r#type: 0,
                message_id: generate_message_id(),
                payload: format!("欢迎 {username} 加入协商聊天室！").into_bytes(),
                metadata: HashMap::new(),
                seq: 0,
            };
            let frame = frame_with_message_command(welcome, Reliability::AtLeastOnce);
            handle.send_to(connection_id, &frame).await.ok();
        }

        Ok(())
    }

    async fn on_disconnect(&self, connection_id: &str) -> Result<()> {
        let username = self
            .usernames
            .lock()
            .await
            .remove(connection_id)
            .unwrap_or_else(|| "未知用户".into());
        info!(%connection_id, %username, "连接断开");
        Ok(())
    }
}
