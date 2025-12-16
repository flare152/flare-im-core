//! # Hook引擎应用启动器
//!
//! 负责依赖注入和服务启动

use std::net::SocketAddr;

use anyhow::{Context, Result};
use tracing::{error, info};

use flare_server_core::runtime::ServiceRuntime;

use super::wire;

/// Hook引擎配置
#[derive(Debug, Clone)]
pub struct HookEngineConfig {
    /// 配置文件路径（可选，最低优先级）
    pub config_file: Option<std::path::PathBuf>,
    /// 数据库URL（可选，最高优先级，用于动态API配置）
    pub database_url: Option<String>,
    /// 配置中心端点（可选，etcd://host:port 或 consul://host:port，中等优先级）
    pub config_center_endpoint: Option<String>,
    /// 租户ID（可选，用于多租户场景）
    pub tenant_id: Option<String>,
    /// 执行模式（串行/并发）
    pub execution_mode: crate::domain::model::ExecutionMode,
    /// 配置刷新间隔（秒）
    pub refresh_interval_secs: u64,
}

impl Default for HookEngineConfig {
    fn default() -> Self {
        Self {
            config_file: Some(std::path::PathBuf::from("config/hooks.toml")),
            database_url: None,
            config_center_endpoint: None,
            tenant_id: None,
            execution_mode: crate::domain::model::ExecutionMode::Sequential,
            refresh_interval_secs: 60,
        }
    }
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: HookEngineConfig) -> Result<()> {
        use flare_im_core::{ServiceHelper, load_config};

        // 加载应用配置
        let app_config = load_config(Some("config"));

        // 解析服务器地址（使用默认端口 50110）
        let default_port = 50110;
        let address: SocketAddr = format!(
            "{}:{}",
            app_config.base().server.address,
            app_config.base().server.port
        )
        .parse()
        .context("invalid hook-engine server address")?;
        info!(address = %address, "Server address parsed successfully");

        // 使用 Wire 风格的依赖注入构建应用上下文
        let context = wire::initialize(config).await?;

        info!("ApplicationBootstrap created successfully");

        // 运行服务
        Self::run_with_context(context, address).await
    }

    /// 运行服务（带应用上下文）
    async fn run_with_context(
        context: wire::ApplicationContext,
        address: SocketAddr,
    ) -> Result<()> {
        use tonic::transport::Server;

        info!(
            address = %address,
            port = %address.port(),
            "Starting Hook Engine gRPC service..."
        );

        // 使用 ServiceRuntime 管理服务生命周期
        let address_clone = address;
        let hook_extension_service = context.hook_extension_service;
        let hook_service = context.hook_service;

        let runtime = ServiceRuntime::new("hook-engine", address)
            .add_spawn_with_shutdown("hook-engine-grpc", move |shutdown_rx| async move {
                // 构建服务器（使用链式调用）
                let mut server_builder = Server::builder()
                    .add_service(
                        flare_proto::hooks::hook_extension_server::HookExtensionServer::new(
                            hook_extension_service
                        ),
                    );

                // 注册HookService服务（如果可用）
                if let Some(hook_service) = hook_service {
                    server_builder = server_builder.add_service(
                        flare_proto::hooks::hook_service_server::HookServiceServer::new(
                            hook_service
                        ),
                    );
                    info!("HookService registered");
                }

                server_builder
                    .serve_with_shutdown(address_clone, async move {
                        info!(
                            address = %address_clone,
                            port = %address_clone.port(),
                            "✅ Hook Engine gRPC service is listening"
                        );

                        // 同时监听 Ctrl+C 和关闭通道
                        tokio::select! {
                            _ = tokio::signal::ctrl_c() => {
                                tracing::info!("shutdown signal received (Ctrl+C)");
                            }
                            _ = shutdown_rx => {
                                tracing::info!("shutdown signal received (service registration failed)");
                            }
                        }
                    })
                    .await
                    .map_err(|e| format!("gRPC server error: {}", e).into())
            });

        // 运行服务（带服务注册）
        runtime
            .run_with_registration(|addr| {
                Box::pin(async move {
                    // 注册服务（使用常量）
                    use flare_im_core::service_names::HOOK_ENGINE;
                    match flare_im_core::discovery::register_service_only(HOOK_ENGINE, addr, None)
                        .await
                    {
                        Ok(Some(registry)) => {
                            info!("✅ Service registered: {}", HOOK_ENGINE);
                            Ok(Some(registry))
                        }
                        Ok(None) => {
                            info!("Service discovery not configured, skipping registration");
                            Ok(None)
                        }
                        Err(e) => {
                            error!(
                                error = %e,
                                "❌ Service registration failed"
                            );
                            Err(format!("Service registration failed: {}", e).into())
                        }
                    }
                })
            })
            .await
    }
}
