//! 应用启动器 - 负责依赖注入和服务启动
//!
//! 注意：Push Server 是纯消费者，不提供 gRPC 服务
//! ACK 通过 Push Proxy → Kafka → Push Server 的方式传递

use anyhow::Result;
use tracing::info;

use flare_server_core::runtime::ServiceRuntime;

use super::wire;

pub use wire::ApplicationContext;

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    /// 
    /// 注意：Push Server 是纯消费者，不提供 gRPC 服务
    /// ACK 通过 Push Proxy → Kafka → Push Server 的方式传递
    pub async fn run() -> Result<()> {
        use flare_im_core::load_config;
        
        // 初始化 OpenTelemetry 追踪
        #[cfg(feature = "tracing")]
        {
            let otlp_endpoint = std::env::var("OTLP_ENDPOINT").ok();
            if let Err(e) = flare_im_core::tracing::init_tracing("push-server", otlp_endpoint.as_deref()) {
                tracing::error!(error = %e, "Failed to initialize OpenTelemetry tracing");
            } else {
                info!("✅ OpenTelemetry tracing initialized");
            }
        }
        
        // 加载应用配置
        let app_config = load_config(Some("./config"));
        
        // 使用 Wire 风格的依赖注入构建应用上下文
        let context = wire::initialize(app_config).await?;
        
        info!("ApplicationBootstrap created successfully");
        
        // 运行服务（纯消费者，只启动 Kafka 消费者）
        Self::run_with_context(context).await
    }

    /// 运行服务（带应用上下文）
    /// 
    /// 注意：Push Server 是纯消费者，不提供 gRPC 服务
    /// 只启动 Kafka 消费者（推送消息消费者 + ACK 消费者）
    pub async fn run_with_context(context: ApplicationContext) -> Result<()> {
        let consumer = context.consumer;
        let ack_consumer = context.ack_consumer;

        info!("Starting Push Server (Kafka consumers only, no gRPC service)...");

        // 使用 ServiceRuntime 管理 Kafka 消费者（纯消费者模式，不需要地址）
        let runtime = ServiceRuntime::new_consumer_only("push-server")
            // 添加推送消息 Kafka 消费者任务
            .add_consumer("kafka-consumer", async move {
                info!("Starting Push Kafka consumer...");
                consumer.run().await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                        format!("Push Kafka consumer error: {}", e).into()
                    })
            })
            // 添加 ACK Kafka 消费者任务
            .add_consumer("ack-kafka-consumer", async move {
                info!("Starting ACK Kafka consumer...");
                ack_consumer.run().await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                        format!("ACK Kafka consumer error: {}", e).into()
                    })
            });

        // 运行服务（不带服务注册，因为这是纯消费者服务）
        runtime.run().await
    }
}

