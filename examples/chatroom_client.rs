//! # 聊天室客户端示例
//!
//! 这是一个基于 Flare IM Core 的聊天室客户端示例，连接到 `flare-signaling-gateway`，
//! 支持多人同时在线聊天。所有消息都发送到同一个聊天室（session_id: "chatroom"），只支持文本消息。
//!
//! ## 使用方法
//!
//! ### 基本使用
//!
//! ```bash
//! # 启动客户端（使用默认用户ID）
//! cargo run --example chatroom_client
//!
//! # 指定用户ID
//! cargo run --example chatroom_client -- user1
//!
//! # 使用环境变量指定用户ID
//! USER_ID=user1 cargo run --example chatroom_client
//! ```
//!
//! ### 跨地区网关路由（多网关部署）
//!
//! ```bash
//! # 连接到北京网关
//! NEGOTIATION_HOST=gateway-beijing.example.com:60051 cargo run --example chatroom_client -- user1
//!
//! # 连接到上海网关
//! NEGOTIATION_HOST=gateway-shanghai.example.com:60051 cargo run --example chatroom_client -- user2
//!
//! # 连接到本地网关（开发环境）
//! NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1
//! NEGOTIATION_HOST=localhost:60052 cargo run --example chatroom_client -- user2
//! ```
//!
//! ### 工作原理
//!
//! 1. **客户端连接**：客户端通过 `NEGOTIATION_HOST` 连接到指定的 Access Gateway
//! 2. **网关注册**：Access Gateway 在用户登录时，将 `gateway_id` 注册到 Signaling Online 服务
//! 3. **消息路由**：当业务系统推送消息时，通过 Signaling Online 查询用户所在的 `gateway_id`，然后路由到对应的 Access Gateway
//! 4. **跨地区推送**：支持用户在不同地区的网关之间接收消息

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
use prost::Message;
use tokio::io::{AsyncBufReadExt, BufReader};
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .init();

    // 从环境变量或命令行参数获取配置
    // 支持多网关连接：通过 NEGOTIATION_HOST 指定不同的网关地址
    // 示例：
    //   NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1  # 连接到网关1
    //   NEGOTIATION_HOST=localhost:60052 cargo run --example chatroom_client -- user2  # 连接到网关2
    //   NEGOTIATION_HOST=gateway-beijing.example.com:60051 cargo run --example chatroom_client -- user1  # 连接到北京网关
    let default_host = std::env::var("NEGOTIATION_HOST")
        .unwrap_or_else(|_| "localhost:60051".to_string());
    let default_ws = format!("ws://{default_host}");
    let default_quic = format!("quic://{}", default_host.replace("60051", "60052"));

    let host = std::env::var("NEGOTIATION_HOST").unwrap_or(default_host);
    let ws_url = std::env::var("NEGOTIATION_WS_URL").unwrap_or(default_ws);
    let _quic_url = std::env::var("NEGOTIATION_QUIC_URL").unwrap_or(default_quic);  // 保留但不使用，避免警告
    
    // 显示连接的网关信息（用于跨地区路由调试）
    if let Ok(gateway_id) = std::env::var("GATEWAY_ID") {
        info!("🌍 连接到网关: {}", gateway_id);
    }

    let platform = std::env::var("DEVICE_PLATFORM")
        .map(|value| DevicePlatform::from_str(&value))
        .unwrap_or(DevicePlatform::PC);

    let device_info = DeviceInfo::new(
        format!(
            "chatroom-client-{}-{}",
            platform.as_str(),
            std::process::id()
        ),
        platform.clone(),
    )
    .with_model(platform.as_str().to_string())
    .with_app_version("1.0.0".to_string());

    let user_id = resolve_user_id().await;
    info!(
        %user_id,
        platform = %platform.as_str(),
        host = %host,
        "🚀 启动聊天室客户端"
    );

    let heartbeat = HeartbeatConfig::default()
        .with_interval(Duration::from_secs(30))
        .with_timeout(Duration::from_secs(90));

    let observer = Arc::new(ChatObserver {
        message_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        user_id: user_id.clone(),
        seen_message_ids: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
    });
    let event_handler = Arc::new(ChatEventHandler);

    // 获取 token（从环境变量或生成测试 token）
    let token = std::env::var("TOKEN").unwrap_or_else(|_| {
        // 如果没有提供 token，生成一个测试 token
        use flare_server_core::TokenService;
        let token_service = TokenService::new(
            "insecure-secret".to_string(),
            "flare-im-core".to_string(),
            3600,
        );
        match token_service.generate_token(&user_id, None, None) {
            Ok(t) => {
                info!("🔑 自动生成测试 token");
                t
            }
            Err(e) => {
                warn!(?e, "无法生成 token，连接可能失败");
                String::new()
            }
        }
    });

    // 使用 ws_url 作为基础地址（协议竞速需要完整 URL）
    let mut client_builder = ObserverClientBuilder::new(&ws_url)
        .with_observer(observer.clone() as Arc<dyn ConnectionObserver>)
        .with_event_handler(event_handler as Arc<dyn ClientEventHandler>)
        .with_protocol_race(vec![TransportProtocol::WebSocket])  // 只使用 WebSocket，避免协议竞速超时
        .with_protocol_url(TransportProtocol::WebSocket, ws_url.clone())
        .with_format(flare_core::common::protocol::SerializationFormat::Json)
        .with_compression(CompressionAlgorithm::None)
        .with_device_info(device_info)
        .with_user_id(user_id.clone())
        .with_heartbeat(heartbeat)
        .with_connect_timeout(Duration::from_secs(10))
        .with_reconnect_interval(Duration::from_secs(3))
        .with_max_reconnect_attempts(Some(5));
    
    // 如果提供了 token，添加到客户端配置
    if !token.is_empty() {
        client_builder = client_builder.with_token(token);
    }
    
    let mut client = client_builder.build_with_race().await?;

    info!("✅ 已连接到 {host}");
    info!("   输入聊天内容后回车即可发送，输入 'quit' 或 'exit' 退出");
    info!("   输入 '/userid' 查看当前用户ID");
    info!("   输入 '/help' 查看帮助");

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

                        // 处理命令
                        match message.as_str() {
                            "quit" | "exit" => {
                                info!("退出客户端");
                                break;
                            }
                            "/userid" => {
                                info!("当前用户ID: {user_id}");
                                continue;
                            }
                            "/help" => {
                                print_help();
                                continue;
                            }
                            _ => {}
                        }

                        // 发送消息（统一使用 "chatroom" 作为 session_id，确保所有消息都发送到同一个聊天室）
                        let mut metadata = std::collections::HashMap::new();
                        metadata.insert("session_id".to_string(), "chatroom".as_bytes().to_vec());
                        metadata.insert("message_type".to_string(), "text".as_bytes().to_vec()); // 只发送文本消息
                        
                        let cmd = send_message(
                            generate_message_id(),
                            message.into_bytes(),
                            Some(metadata),
                            None,
                        );
                        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);
                        match client.send_frame(&frame).await {
                            Ok(_) => {
                                debug!("消息已发送");
                            }
                            Err(err) => {
                                error!(?err, "发送消息失败");
                                println!("\n❌ 发送消息失败: {}", err);
                                println!("   请检查网络连接或稍后重试");
                            }
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
                    warn!("连接已断开，尝试重连...");
                    // 客户端会自动重连
                }
            }
        }
    }

    client.disconnect().await?;
    info!("客户端已断开");
    Ok(())
}

fn print_help() {
    println!();
    println!("=== 聊天室客户端帮助 ===");
    println!("命令:");
    println!("  /userid    - 显示当前用户ID");
    println!("  /help      - 显示此帮助信息");
    println!("  quit/exit  - 退出客户端");
    println!();
    println!("使用:");
    println!("  直接输入消息内容后回车即可发送");
    println!("  消息会广播给所有在线的用户");
    println!();
}

async fn resolve_user_id() -> String {
    // 1. 优先使用命令行参数
    if let Some(arg) = std::env::args().nth(1) {
        info!("📝 使用命令行提供的用户ID: {arg}");
        return arg;
    }

    // 2. 使用环境变量
    if let Ok(env_user) = std::env::var("USER_ID") {
        info!("📝 使用环境变量 USER_ID: {env_user}");
        return env_user;
    }

    // 3. 交互式输入
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

/// 聊天室消息观察者
struct ChatObserver {
    message_count: Arc<std::sync::atomic::AtomicU64>,
    user_id: String,
    // 用于去重的消息ID集合（使用简单的 HashSet，限制大小避免内存泄漏）
    seen_message_ids: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

#[async_trait]
impl ConnectionObserver for ChatObserver {
    fn on_event(&self, event: &ConnectionEvent) {
        match event {
            ConnectionEvent::Connected => {
                info!("✅ 已连接到服务器，协商信息已发送");
                info!("   用户ID: {}", self.user_id);
            }
            ConnectionEvent::Disconnected(reason) => {
                warn!("🔴 连接断开: {reason}");
            }
            ConnectionEvent::Error(err) => {
                error!(?err, "连接错误");
            }
            ConnectionEvent::Message(data) => {
                // 使用 Protobuf 解析器（服务端使用 Protobuf 格式）
                let parser = MessageParser::protobuf();
                match parser.parse(data) {
                    Ok(frame) => {
                        if let Some(cmd) = &frame.command {
                            // 处理通知命令（错误提示等）
                            if let Some(Type::Notification(notif)) = &cmd.r#type {
                                if let Ok(notif_type) = NotifType::try_from(notif.r#type) {
                                    match notif_type {
                                        NotifType::Alert => {
                                            // 警告/错误通知
                                            let title = notif.title.clone();
                                            let content = String::from_utf8_lossy(&notif.content);
                                            println!("\n⚠️  [错误] {}: {}", title, content);
                                            
                                            // 如果有原始消息ID，显示
                                            if let Some(msg_id_bytes) = notif.metadata.get("original_message_id") {
                                                if let Ok(msg_id) = String::from_utf8(msg_id_bytes.clone()) {
                                                    println!("   原始消息ID: {}", msg_id);
                                                }
                                            }
                                        }
                                        NotifType::System => {
                                            // 系统通知
                                            let title = notif.title.clone();
                                            let content = String::from_utf8_lossy(&notif.content);
                                            info!("[系统通知] {}: {}", title, content);
                                        }
                                        _ => {
                                            // 其他类型的通知
                                            let title = notif.title.clone();
                                            let content = String::from_utf8_lossy(&notif.content);
                                            info!("[通知] {}: {}", title, content);
                                        }
                                    }
                                }
                                return; // 通知已处理，不继续处理消息
                            }
                            
                            if let Some(Type::Message(msg)) = &cmd.r#type {
                                let index = self
                                    .message_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                
                                // 尝试解析消息内容
                                // Access Gateway 发送的 payload 是序列化后的 Message (common.v1.Message)
                                let (sender, content_text, _message_id) = match flare_proto::common::Message::decode(msg.payload.as_slice()) {
                                    Ok(message) => {
                                        let sender = message.sender_id.clone();
                                        let message_id = message.id.clone();
                                        
                                        // 检查消息是否已经处理过（去重）
                                        {
                                            let mut seen_ids = self.seen_message_ids.lock().unwrap();
                                            if seen_ids.contains(&message_id) {
                                                // 消息已处理过，跳过
                                                debug!("跳过重复消息: {}", message_id);
                                                return;
                                            }
                                            // 添加到已处理集合（限制大小，避免内存泄漏）
                                            if seen_ids.len() > 1000 {
                                                seen_ids.clear(); // 简单清理策略
                                            }
                                            seen_ids.insert(message_id.clone());
                                        }
                                        
                                        // 从 MessageContent 中提取文本内容
                                        // MessageContent 的 oneof 在 prost 中会生成 message_content::Content 枚举
                                        let content_text = if let Some(ref content) = message.content {
                                            // 使用 match 匹配 Content 枚举（只处理文本消息）
                                            match &content.content {
                                                Some(flare_proto::common::message_content::Content::Text(text_content)) => {
                                                    // 文本消息：提取 text 字段
                                                    text_content.text.clone()
                                                }
                                                Some(flare_proto::common::message_content::Content::Image(_)) => {
                                                    "[图片消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Video(_)) => {
                                                    "[视频消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Audio(_)) => {
                                                    "[语音消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::File(_)) => {
                                                    "[文件消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Location(_)) => {
                                                    "[位置消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Card(_)) => {
                                                    "[名片消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Notification(_)) => {
                                                    "[通知消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Custom(_)) => {
                                                    "[自定义消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Forward(_)) => {
                                                    "[转发消息]".to_string()
                                                }
                                                Some(flare_proto::common::message_content::Content::Typing(_)) => {
                                                    "[正在输入]".to_string()
                                                }
                                                None => {
                                                    // content.content 为空，尝试从原始 payload 提取可读文本
                                                    String::from_utf8_lossy(&msg.payload)
                                                        .chars()
                                                        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || "，。！？：；、".contains(*c))
                                                        .take(200)
                                                        .collect::<String>()
                                                        .trim()
                                                        .to_string()
                                                }
                                            }
                                        } else {
                                            // content 为空，尝试直接解析 payload 为 UTF-8 文本
                                            String::from_utf8_lossy(&msg.payload)
                                                .trim()
                                                .to_string()
                                        };
                                        
                                        (sender, content_text, message_id)
                                    }
                                    Err(_) => {
                                        // 如果 Protobuf 解析失败，尝试直接作为 UTF-8 文本
                                        // 使用消息命令的 message_id 作为去重标识
                                        let message_id = msg.message_id.clone();
                                        
                                        // 检查消息是否已经处理过（去重）
                                        {
                                            let mut seen_ids = self.seen_message_ids.lock().unwrap();
                                            if seen_ids.contains(&message_id) {
                                                // 消息已处理过，跳过
                                                debug!("跳过重复消息: {}", message_id);
                                                return;
                                            }
                                            // 添加到已处理集合
                                            if seen_ids.len() > 1000 {
                                                seen_ids.clear();
                                            }
                                            seen_ids.insert(message_id.clone());
                                        }
                                        
                                        let text = String::from_utf8_lossy(&msg.payload)
                                            .trim()
                                            .to_string();
                                        ("未知".to_string(), text, message_id)
                                    }
                                };
                                
                                // 格式化输出
                                let formatted_text = if sender == self.user_id {
                                    format!("[我] {content_text}")
                                } else {
                                    format!("[{}] {content_text}", sender)
                                };
                                
                                // 打印格式化的消息（使用 println! 确保输出到控制台）
                                if sender == self.user_id {
                                    // 自己的消息用 debug 级别，避免重复显示
                                    debug!("[消息 #{}] {}", index, formatted_text);
                                } else {
                                    // 其他人的消息用 println! 清晰显示
                                    println!("\n📨 [消息 #{}] {}", index, formatted_text);
                                }
                            }
                        }
                    }
                    Err(err) => {
                        // 如果 Protobuf 解析失败，尝试 JSON
                        let json_parser = MessageParser::json();
                        match json_parser.parse(data) {
                            Ok(frame) => {
                                if let Some(cmd) = &frame.command {
                                    if let Some(Type::Message(msg)) = &cmd.r#type {
                                        let text = String::from_utf8_lossy(&msg.payload).trim().to_string();
                                        let index = self
                                            .message_count
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                            + 1;
                                        println!("\n📨 [消息 #{}] {}", index, text);
                                    }
                                }
                            }
                            Err(_) => {
                                error!(?err, "解析消息失败（Protobuf 和 JSON 都失败）");
                            }
                        }
                    }
                }
            }
        }
    }
}

/// 聊天室事件处理器
struct ChatEventHandler;

#[async_trait]
impl ClientEventHandler for ChatEventHandler {
    async fn handle_system_command(
        &self,
        command_type: SysType,
        frame: &Frame,
    ) -> Result<Option<Frame>> {
        debug!("[系统] {:?}", command_type);
        if let Some(cmd) = &frame.command {
            if let Some(Type::System(sys)) = &cmd.r#type {
                if let Some(format_bytes) = sys.metadata.get("format") {
                    if let Ok(value) = String::from_utf8(format_bytes.clone()) {
                        debug!("   format: {value}");
                    }
                }
                if let Some(compression_bytes) = sys.metadata.get("compression") {
                    if let Ok(value) = String::from_utf8(compression_bytes.clone()) {
                        debug!("   compression: {value}");
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
            ConnectionEvent::Message(_) => {
                // 消息已经在 ChatObserver::on_event 中处理，这里不重复处理
                // 避免消息被显示两次
            }
        }
        Ok(())
    }
}

