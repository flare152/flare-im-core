//! # 一对一聊天客户端示例
//!
//! 这是一个基于 Flare IM Core 的一对一聊天客户端示例，连接到 `flare-signaling-gateway`，
//! 支持两人之间的私聊。消息直接发送给指定的接收方，不经过聊天室广播。
//!
//! ## 使用方法
//!
//! ### 基本使用
//!
//! ```bash
//! # 启动客户端（使用默认用户ID）
//! cargo run --example chatroom_client
//!
//! # 指定用户ID和接收方ID
//! cargo run --example chatroom_client -- user1 user2
//!
//! # 使用环境变量指定用户ID和接收方ID
//! USER_ID=user1 RECIPIENT_ID=user2 cargo run --example chatroom_client
//! ```
//!
//! ### 跨地区网关路由（多网关部署）
//!
//! ```bash
//! # 连接到北京网关
//! NEGOTIATION_HOST=gateway-beijing.example.com:60051 cargo run --example chatroom_client -- user1 user2
//!
//! # 连接到上海网关
//! NEGOTIATION_HOST=gateway-shanghai.example.com:60051 cargo run --example chatroom_client -- user1 user2
//!
//! # 连接到本地网关（开发环境）
//! NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1 user2
//! ```
//!
//! ### 工作原理
//!
//! 1. **客户端连接**：客户端通过 `NEGOTIATION_HOST` 连接到指定的 Access Gateway
//! 2. **网关注册**：Access Gateway 在用户登录时，将 `gateway_id` 注册到 Signaling Online 服务
//! 3. **消息路由**：消息通过 Signaling Online 查询接收方所在的 `gateway_id`，然后路由到对应的 Access Gateway
//! 4. **点对点通信**：消息直接发送给指定接收方，不经过聊天室广播

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
    //   NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1 user2  # 连接到网关1
    let default_host = std::env::var("NEGOTIATION_HOST")
        .unwrap_or_else(|_| "localhost:60051".to_string());
    let default_ws = format!("ws://{default_host}");

    let host = std::env::var("NEGOTIATION_HOST").unwrap_or(default_host);
    let ws_url = std::env::var("NEGOTIATION_WS_URL").unwrap_or(default_ws);
    
    let platform = std::env::var("DEVICE_PLATFORM")
        .map(|value| DevicePlatform::from_str(&value))
        .unwrap_or(DevicePlatform::PC);

    let device_info = DeviceInfo::new(
        format!(
            "p2p-client-{}-{}",
            platform.as_str(),
            std::process::id()
        ),
        platform.clone(),
    )
    .with_model(platform.as_str().to_string())
    .with_app_version("1.0.0".to_string());

    // 解析用户ID和接收方ID
    let (user_id, recipient_id) = resolve_user_and_recipient_id().await;
    info!(
        %user_id,
        %recipient_id,
        platform = %platform.as_str(),
        host = %host,
        "🚀 启动一对一聊天客户端"
    );

    let heartbeat = HeartbeatConfig::default()
        .with_interval(Duration::from_secs(30))
        .with_timeout(Duration::from_secs(90));

    let observer = Arc::new(ChatObserver {
        message_count: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        user_id: user_id.clone(),
        recipient_id: recipient_id.clone(),
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
    info!("   当前用户ID: {user_id}");
    info!("   接收方用户ID: {recipient_id}");
    info!("   输入聊天内容后回车即可发送，输入 'quit' 或 'exit' 退出");
    info!("   输入 '/userid' 查看当前用户ID");
    info!("   输入 '/recipient' 查看接收方ID");
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
                            "/recipient" => {
                                info!("接收方用户ID: {recipient_id}");
                                continue;
                            }
                            "/help" => {
                                print_help();
                                continue;
                            }
                            _ => {}
                        }

                        // 发送一对一消息
                        // 构造消息内容
                        let text_content = flare_proto::common::TextContent {
                            text: message.clone(),
                            mentions: vec![],
                        };
                        
                        let message_content = flare_proto::common::MessageContent {
                            content: Some(flare_proto::common::message_content::Content::Text(text_content)),
                            extensions: vec![],
                        };
                        
                        // 构造完整的Message对象，将recipient_id作为session_id
                        let timestamp = prost_types::Timestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        };
                        
                        // 设置接收方用户ID到attributes中
                        let mut attributes = std::collections::HashMap::new();
                        attributes.insert("recipient_id".to_string(), recipient_id.clone());
                        
                        // 构造符合Message Orchestrator期望的session_id格式
                        // 对于单聊，格式应该是 "single:sender_id:recipient_id"
                        let session_id = format!("single:{}:{}", user_id, recipient_id);
                        
                        let msg = flare_proto::common::Message {
                            id: generate_message_id(),
                            session_id,  // 使用正确的session_id格式
                            client_msg_id: String::new(),
                            sender_id: user_id.clone(),
                            source: flare_proto::common::MessageSource::User as i32,
                            seq: 0,
                            timestamp: Some(timestamp.clone()),
                            session_type: flare_proto::common::SessionType::Single as i32,
                            message_type: flare_proto::common::MessageType::Text as i32,
                            business_type: String::new(),
                            content: Some(message_content),
                            content_type: flare_proto::common::ContentType::PlainText as i32,
                            attachments: vec![],
                            extra: std::collections::HashMap::new(),
                            attributes,
                            status: flare_proto::common::MessageStatus::Created as i32,
                            is_recalled: false,
                            recalled_at: None,
                            recall_reason: String::new(),
                            is_burn_after_read: false,
                            burn_after_seconds: 0,
                            timeline: Some(flare_proto::common::MessageTimeline {
                                created_at: Some(timestamp.clone()),
                                persisted_at: None,
                                delivered_at: None,
                                read_at: None,
                            }),
                            visibility: std::collections::HashMap::new(),
                            read_by: vec![],
                            reactions: vec![],
                            edit_history: vec![],
                            tenant: Some(flare_proto::common::TenantContext {
                                tenant_id: "default".to_string(),
                                business_type: "im".to_string(),
                                environment: "development".to_string(),
                                organization_id: String::new(),
                                labels: std::collections::HashMap::new(),
                                attributes: std::collections::HashMap::new(),
                            }),
                            audit: None,
                            tags: vec![],
                            offline_push_info: None,
                            extensions: vec![],
                        };
                        
                        // 序列化消息对象
                        let mut buf = Vec::new();
                        msg.encode(&mut buf).map_err(|e| flare_core::common::error::FlareError::serialization_error(
                            format!("Failed to encode message: {}", e)
                        ))?;
                        
                        let cmd = send_message(
                            msg.id.clone(),
                            buf,
                            None,
                            None,
                        );
                        let frame = frame_with_message_command(cmd, Reliability::AtLeastOnce);
                        match client.send_frame(&frame).await {
                            Ok(_) => {
                                debug!("消息已发送给 {}", recipient_id);
                                println!("[我 ➡ {}]: {}", recipient_id, message);
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
    println!("=== 一对一聊天客户端帮助 ===");
    println!("命令:");
    println!("  /userid    - 显示当前用户ID");
    println!("  /recipient - 显示接收方用户ID");
    println!("  /help      - 显示此帮助信息");
    println!("  quit/exit  - 退出客户端");
    println!();
    println!("使用:");
    println!("  直接输入消息内容后回车即可发送");
    println!("  消息会直接发送给指定的接收方");
    println!();
}

async fn resolve_user_and_recipient_id() -> (String, String) {
    let args: Vec<String> = std::env::args().collect();
    
    // 1. 优先使用命令行参数
    if args.len() >= 3 {
        info!("📝 使用命令行提供的用户ID: {} 和接收方ID: {}", args[1], args[2]);
        return (args[1].clone(), args[2].clone());
    }
    
    // 2. 使用环境变量
    let user_id = if let Ok(env_user) = std::env::var("USER_ID") {
        info!("📝 使用环境变量 USER_ID: {env_user}");
        env_user
    } else {
        // 交互式输入用户ID
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
    };
    
    let recipient_id = if let Ok(env_recipient) = std::env::var("RECIPIENT_ID") {
        info!("📝 使用环境变量 RECIPIENT_ID: {env_recipient}");
        env_recipient
    } else {
        // 交互式输入接收方ID
        info!("📝 请输入接收方用户ID:");
        print!("接收方用户ID: ");
        use std::io::Write;
        std::io::stdout().flush().unwrap();

        let stdin = tokio::io::stdin();
        let mut reader = BufReader::new(stdin);
        let mut buffer = String::new();
        match reader.read_line(&mut buffer).await {
            Ok(_) => {
                buffer.trim().to_string()
            }
            Err(err) => {
                error!(?err, "读取接收方用户ID失败");
                "unknown".to_string()
            }
        }
    };
    
    (user_id, recipient_id)
}

/// 一对一聊天消息观察者
struct ChatObserver {
    message_count: Arc<std::sync::atomic::AtomicU64>,
    user_id: String,
    recipient_id: String,
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
                info!("   接收方ID: {}", self.recipient_id);
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
                                let _index = self
                                    .message_count
                                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                    + 1;
                                
                                // 添加调试信息
                                debug!("收到消息，payload长度: {}, message_id: {}", msg.payload.len(), msg.message_id);
                                
                                // 尝试解析消息内容
                                // Access Gateway 发送的 payload 是序列化后的 Message (common.v1.Message)
                                let (sender, content_text, _message_id) = match flare_proto::common::Message::decode(msg.payload.as_slice()) {
                                    Ok(message) => {
                                        let sender = message.sender_id.clone();
                                        let message_id = message.id.clone();
                                        
                                        // 添加调试信息
                                        debug!("成功解析消息: sender={}, message_id={}, session_id={}", sender, message_id, message.session_id);
                                        
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
                                            debug!("添加消息到已处理集合: {}", message_id);
                                        }
                                        
                                        // 从 MessageContent 中提取文本内容
                                        // MessageContent 的 oneof 在 prost 中会生成 message_content::Content 枚举
                                        let content_text = if let Some(ref content) = message.content {
                                            // 使用 match 匹配 Content 枚举（只处理文本消息）
                                            match &content.content {
                                                Some(flare_proto::common::message_content::Content::Text(text_content)) => {
                                                    // 文本消息：提取 text 字段
                                                    debug!("解析到文本消息: {}", text_content.text);
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
                                                    debug!("content.content 为空，尝试从原始 payload 提取文本");
                                                    let text = String::from_utf8_lossy(&msg.payload)
                                                        .chars()
                                                        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || "，。！？：；、".contains(*c))
                                                        .take(200)
                                                        .collect::<String>()
                                                        .trim()
                                                        .to_string();
                                                    debug!("从原始 payload 提取到文本: {}", text);
                                                    text
                                                },
                                                _ => {
                                                    // 其他未知类型，尝试直接解析 payload 为 UTF-8 文本
                                                    debug!("未知内容类型，尝试直接解析 payload");
                                                    let text = String::from_utf8_lossy(&msg.payload)
                                                        .trim()
                                                        .to_string();
                                                    debug!("直接解析 payload 得到文本: {}", text);
                                                    text
                                                }
                                            }
                                        } else {
                                            // content 为空，尝试直接解析 payload 为 UTF-8 文本
                                            debug!("content 为空，尝试直接解析 payload 为 UTF-8 文本");
                                            let text = String::from_utf8_lossy(&msg.payload)
                                                .trim()
                                                .to_string();
                                            debug!("解析得到文本: {}", text);
                                            text
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
                                // 只打印清晰的文本内容，避免显示二进制数据
                                // 过滤掉不可打印字符，只保留字母、数字、中文和常见标点
                                let clean_content = content_text
                                    .chars()
                                    .filter(|c| {
                                        c.is_alphanumeric() || 
                                        c.is_whitespace() || 
                                        "，。！？：；、,.!?;:".contains(*c) ||
                                        (c.clone() as u32) > 127  // 保留非ASCII字符（如中文）
                                    })
                                    .collect::<String>()
                                    .trim()
                                    .to_string();
                                
                                if !clean_content.is_empty() {
                                    println!("\n📨 [{sender} ➡ {recipient}]: {content}", 
                                        sender = sender, 
                                        recipient = self.user_id, 
                                        content = clean_content);
                                } else {
                                    // 如果过滤后没有内容，至少显示原始内容的前50个字符
                                    let truncated = content_text.chars().take(50).collect::<String>();
                                    println!("\n📨 [{sender} ➡ {recipient}]: {content}", 
                                        sender = sender, 
                                        recipient = self.user_id, 
                                        content = truncated);
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
                                        let _index = self
                                            .message_count
                                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                                            + 1;
                                        println!("\n📨 [消息 #{}] {}", _index, text);
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
