use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use flare_core::client::{ClientEventHandler, ObserverClientBuilder};
use flare_core::common::MessageParser;
use flare_core::common::compression::CompressionAlgorithm;
use flare_core::common::config_types::{HeartbeatConfig, TransportProtocol};
use flare_core::common::device::{DeviceInfo, DevicePlatform};
use flare_core::common::error::Result;
use flare_core::common::protocol::flare::core::commands::command::Type;
use flare_core::common::protocol::flare::core::commands::message_command::Type as MsgType;
use flare_core::common::protocol::flare::core::commands::notification_command::Type as NotifType;
use flare_core::common::protocol::flare::core::commands::system_command::Type as SysType;
use flare_core::common::protocol::{
    Frame, Reliability, frame_with_message_command, generate_message_id, send_message,
};
use flare_core::transport::events::{ConnectionEvent, ConnectionObserver};
use flare_im_core::config::ServiceRuntimeConfig;
use flare_im_core::load_config;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let app_config = load_config(Some("config"));
    let runtime = app_config
        .compose_service_config(&ServiceRuntimeConfig::default(), "flare.negotiation.chat");

    let default_host = format!("{}:{}", runtime.server.address, runtime.server.port);
    let default_ws = format!("ws://{default_host}");
    let default_quic = format!(
        "quic://{}:{}",
        runtime.server.address,
        runtime.server.port + 1
    );

    let host = std::env::var("NEGOTIATION_HOST").unwrap_or(default_host);
    let ws_url = std::env::var("NEGOTIATION_WS_URL").unwrap_or(default_ws);
    let quic_url = std::env::var("NEGOTIATION_QUIC_URL").unwrap_or(default_quic);

    let platform = std::env::var("DEVICE_PLATFORM")
        .map(|value| DevicePlatform::from_str(&value))
        .unwrap_or(DevicePlatform::PC);

    let device_info = DeviceInfo::new(
        format!(
            "negotiation-client-{}-{}",
            platform.as_str(),
            std::process::id()
        ),
        platform.clone(),
    )
    .with_model(platform.as_str().to_string())
    .with_app_version("1.0.0".to_string());

    let user_id = resolve_user_id().await;
    info!(%user_id, platform = %platform.as_str(), "🚀 启动协商客户端");

    let heartbeat = HeartbeatConfig::default()
        .with_interval(Duration::from_secs(30))
        .with_timeout(Duration::from_secs(90));

    let observer = Arc::new(ChatObserver {
        message_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    });
    let event_handler = Arc::new(DebugEventHandler);

    let mut client = ObserverClientBuilder::new(&host)
        .with_observer(observer.clone() as Arc<dyn ConnectionObserver>)
        .with_event_handler(event_handler as Arc<dyn ClientEventHandler>)
        .with_protocol_race(vec![TransportProtocol::QUIC, TransportProtocol::WebSocket])
        .with_protocol_url(TransportProtocol::WebSocket, ws_url)
        .with_protocol_url(TransportProtocol::QUIC, quic_url)
        .with_format(flare_core::common::protocol::SerializationFormat::Json)
        .with_compression(CompressionAlgorithm::None)
        .with_device_info(device_info)
        .with_user_id(user_id.clone())
        .with_heartbeat(heartbeat)
        .with_connect_timeout(Duration::from_secs(10))
        .with_reconnect_interval(Duration::from_secs(3))
        .with_max_reconnect_attempts(Some(5))
        .build_with_race()
        .await?;

    info!("✅ 已连接到 {host}");
    info!("   输入聊天内容后回车即可发送，输入 'quit' 退出");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    loop {
        tokio::select! {
            read = reader.read_line(&mut line) => {
                match read {
                    Ok(0) => {
                        info!("输入结束，退出客户端");
                        break;
                    }
                    Ok(_) => {
                        let message = line.trim().to_string();
                        line.clear();

                        if message.is_empty() {
                            continue;
                        }

                        if matches!(message.as_str(), "quit" | "exit") {
                            info!("退出客户端");
                            break;
                        }

                        if message == "/userid" {
                            info!("当前用户ID: {user_id}");
                            continue;
                        }

                        if message == "/platform" {
                            info!("当前平台: {}", platform.as_str());
                            continue;
                        }

                        let cmd = send_message(
                            generate_message_id(),
                            message.into_bytes(),
                            None,
                            None,
                        );
                        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);
                        if let Err(err) = client.send_frame(&frame).await {
                            error!(?err, "发送消息失败");
                        }
                    }
                    Err(err) => {
                        error!(?err, "读取输入失败");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                if !client.is_connected() {
                    warn!("连接已断开");
                    break;
                }
            }
        }
    }

    client.disconnect().await?;
    info!("客户端已断开");
    Ok(())
}

async fn resolve_user_id() -> String {
    if let Some(arg) = std::env::args().nth(1) {
        info!("📝 使用命令行提供的用户ID: {arg}");
        return arg;
    }

    if let Ok(env_user) = std::env::var("USER_ID") {
        info!("📝 使用环境变量 USER_ID: {env_user}");
        return env_user;
    }

    info!("📝 请输入用户ID（直接回车使用默认值）:");
    print!("用户ID (默认: user-{}): ", std::process::id());
    use std::io::Write;
    std::io::stdout().flush().unwrap();

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut buffer = String::new();
    match reader.read_line(&mut buffer).await {
        Ok(_) => {
            let trimmed = buffer.trim();
            if trimmed.is_empty() {
                format!("user-{}", std::process::id())
            } else {
                trimmed.to_string()
            }
        }
        Err(err) => {
            error!(?err, "读取用户输入失败，使用默认用户ID");
            format!("user-{}", std::process::id())
        }
    }
}

struct ChatObserver {
    message_count: Arc<std::sync::atomic::AtomicU64>,
}

#[async_trait]
impl ConnectionObserver for ChatObserver {
    fn on_event(&self, event: &ConnectionEvent) {
        match event {
            ConnectionEvent::Connected => {
                info!("✅ 已连接到服务器，协商信息已发送");
            }
            ConnectionEvent::Disconnected(reason) => {
                warn!("🔴 连接断开: {reason}");
            }
            ConnectionEvent::Error(err) => {
                error!(?err, "连接错误");
            }
            ConnectionEvent::Message(data) => {
                let parser = MessageParser::json();
                match parser.parse(data) {
                    Ok(frame) => {
                        if let Some(cmd) = &frame.command {
                            if let Some(Type::Message(msg)) = &cmd.r#type {
                                let text = String::from_utf8_lossy(&msg.payload);
                                let index = self
                                    .message_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                info!("[消息 #{index}] {text}");
                            }
                        }
                    }
                    Err(err) => error!(?err, "解析消息失败"),
                }
            }
        }
    }
}

struct DebugEventHandler;

#[async_trait]
impl ClientEventHandler for DebugEventHandler {
    async fn handle_system_command(
        &self,
        command_type: SysType,
        frame: &Frame,
    ) -> Result<Option<Frame>> {
        info!("[系统] {:?}", command_type);
        if let Some(cmd) = &frame.command {
            if let Some(Type::System(sys)) = &cmd.r#type {
                if let Some(format_bytes) = sys.metadata.get("format") {
                    if let Ok(value) = String::from_utf8(format_bytes.clone()) {
                        info!("   format: {value}");
                    }
                }
                if let Some(compression_bytes) = sys.metadata.get("compression") {
                    if let Ok(value) = String::from_utf8(compression_bytes.clone()) {
                        info!("   compression: {value}");
                    }
                }
            }
        }
        Ok(None)
    }

    async fn handle_message_command(
        &self,
        command_type: MsgType,
        _: &Frame,
    ) -> Result<Option<Frame>> {
        debug!("[消息命令] {:?}", command_type);
        Ok(None)
    }

    async fn handle_notification_command(
        &self,
        command_type: NotifType,
        _: &Frame,
    ) -> Result<Option<Frame>> {
        debug!("[通知命令] {:?}", command_type);
        Ok(None)
    }

    async fn handle_connection_event(&self, event: &ConnectionEvent) -> Result<()> {
        match event {
            ConnectionEvent::Connected => info!("[事件] 已连接"),
            ConnectionEvent::Disconnected(reason) => warn!("[事件] 断开: {reason}"),
            ConnectionEvent::Error(err) => error!("[事件] 错误: {err:?}"),
            ConnectionEvent::Message(_) => {}
        }
        Ok(())
    }
}
