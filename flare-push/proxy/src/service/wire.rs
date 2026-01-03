//! Wire 风格的依赖注入模块
//!
//! 类似 Go 的 Wire 框架，提供简单的依赖构建方法

use std::sync::Arc;

use anyhow::{Context, Result};

use crate::application::handlers::PushCommandHandler;
use crate::domain::repositories::PushEventPublisher;
use crate::domain::service::PushDomainService;
use crate::infrastructure::config::PushProxyConfig;
use crate::infrastructure::messaging::kafka_publisher::KafkaPushEventPublisher;
use crate::interfaces::grpc::handler::PushGrpcHandler;

use flare_im_core::hooks::HookDispatcher;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub handler: PushGrpcHandler,
    pub hook_dispatcher: HookDispatcher,
}

/// 构建应用上下文
///
/// 类似 Go Wire 的 Initialize 函数，按照依赖顺序构建所有组件
///
/// # 参数
/// * `app_config` - 应用配置
///
/// # 返回
/// * `ApplicationContext` - 构建好的应用上下文
pub async fn initialize(
    app_config: &flare_im_core::config::FlareAppConfig,
) -> Result<ApplicationContext> {
    // 1. 加载代理配置
    let proxy_config = Arc::new(PushProxyConfig::from_app_config(app_config));

    // 2. 构建事件发布器
    let publisher: Arc<dyn PushEventPublisher> = Arc::new(
        KafkaPushEventPublisher::new(proxy_config.clone())
            .context("Failed to create Kafka push event publisher")?,
    );

    // 3. 构建请求验证器
    let validator = Arc::new(crate::infrastructure::validator::RequestValidatorImpl::new());

    // 4. 初始化 Hook 调度器
    let hook_dispatcher = HookDispatcher::new(flare_im_core::hooks::GlobalHookRegistry::get());

    // 5. 构建领域服务
    let domain_service = Arc::new(PushDomainService::new(
        publisher,
        validator,
        hook_dispatcher.clone(),
    ));

    // 6. 构建命令处理器
    let command_handler = Arc::new(PushCommandHandler::new(domain_service));

    // 7. 构建 gRPC 处理器
    let handler = PushGrpcHandler::new(command_handler);

    Ok(ApplicationContext {
        handler,
        hook_dispatcher,
    })
}
