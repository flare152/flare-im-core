//! 应用启动器 - 负责依赖注入和服务启动

use anyhow::Result;
use tracing::info;

use flare_server_core::runtime::ServiceRuntime;

use super::wire;

pub use wire::ApplicationContext;

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
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
        
        // 运行服务
        Self::run_with_context(context).await
    }

    /// 运行服务（带应用上下文）
    pub async fn run_with_context(context: ApplicationContext) -> Result<()> {
        info!("Starting Push Server (Kafka consumer)");

        // 使用 ServiceRuntime 管理消费者（不需要地址）
        let consumer = context.consumer;
        let runtime = ServiceRuntime::new_consumer_only("push-server")
            .add_consumer("kafka-consumer", async move {
                // 运行消费者循环
                consumer.run().await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                        format!("Kafka consumer error: {}", e).into()
                    })
            });

        // 运行服务（不带服务注册，因为这是消费者服务）
        runtime.run().await
    }
}

