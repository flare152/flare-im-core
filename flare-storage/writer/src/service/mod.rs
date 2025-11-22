//! 服务模块 - 包含服务启动、注册和管理相关功能

use std::sync::Arc;

use anyhow::Result;
use tracing::info;

use flare_server_core::runtime::ServiceRuntime;

mod wire;

pub use wire::ApplicationContext;

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run() -> Result<()> {
        use flare_im_core::load_config;
        
        // 加载应用配置
        let app_config = load_config(Some("config"));
        
        // 使用 Wire 风格的依赖注入构建应用上下文
        let context = self::wire::initialize(app_config).await?;
        
        info!("ApplicationBootstrap created successfully");
        
        // 运行服务
        Self::run_with_context(context).await
    }

    /// 运行服务（带应用上下文）
    ///
    /// 使用 ServiceRuntime 管理消费者生命周期，支持优雅停机
    /// 支持添加多个消费者任务
    pub async fn run_with_context(context: ApplicationContext) -> Result<()> {
        let consumer = Arc::new(context.consumer);
        
        // 使用 ServiceRuntime 管理消费者（不需要地址）
        let runtime = ServiceRuntime::new_consumer_only("storage-writer")
            .add_consumer("kafka-consumer", async move {
                // 运行消费者循环
                consumer.consume_messages().await
                    .map_err(|e| format!("Kafka consumer error: {}", e).into())
            });

        // 运行服务（不带服务注册，因为这是消费者服务）
        runtime.run().await
    }
}
