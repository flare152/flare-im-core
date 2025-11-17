//! # Hook引擎应用启动器
//!
//! 负责依赖注入和服务启动

use anyhow::Result;
use std::sync::Arc;

use crate::application::commands::HookCommandService;
use crate::application::queries::HookQueryService;
use crate::domain::service::HookOrchestrationService;
use crate::infrastructure::adapters::HookAdapterFactory;
use crate::infrastructure::monitoring::{ExecutionRecorder, MetricsCollector};

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
    pub execution_mode: crate::domain::models::ExecutionMode,
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
            execution_mode: crate::domain::models::ExecutionMode::Sequential,
            refresh_interval_secs: 60,
        }
    }
}

/// Hook引擎主接口
pub struct HookEngine {
    /// Hook命令服务
    pub command_service: Arc<HookCommandService>,
    /// Hook查询服务
    pub query_service: Arc<HookQueryService>,
    /// 配置监听器
    pub config_watcher: Arc<crate::infrastructure::config::ConfigWatcher>,
    /// 指标收集器
    pub metrics_collector: Arc<MetricsCollector>,
    /// 执行记录器
    pub execution_recorder: Arc<ExecutionRecorder>,
    /// Hook注册表
    pub registry: Arc<crate::service::registry::CoreHookRegistry>,
    /// 应用服务
    pub application_service: Arc<crate::application::service::HookApplicationService>,
    /// Hook配置仓储（用于HookService）
    pub config_repository: Option<Arc<crate::infrastructure::persistence::postgres_config::PostgresHookConfigRepository>>,
}

impl HookEngine {
    /// 创建Hook引擎
    pub async fn new(config: HookEngineConfig) -> Result<Self> {
        // 创建配置加载器（按优先级从低到高）
        let mut loaders: Vec<Arc<dyn crate::infrastructure::config::ConfigLoader>> = Vec::new();
        
        // 1. 配置文件（最低优先级）
        if let Some(ref path) = config.config_file {
            loaders.push(Arc::new(crate::infrastructure::config::FileConfigLoader::new(path.clone())));
        }
        
        // 2. 配置中心（中等优先级）
        if let Some(ref endpoint) = config.config_center_endpoint {
            loaders.push(Arc::new(crate::infrastructure::config::ConfigCenterLoader::new(
                endpoint.clone(),
                config.tenant_id.clone(),
            )));
        }
        
        // 3. 数据库配置（最高优先级）
        let config_repository = if let Some(ref database_url) = config.database_url {
            let repository = Arc::new(
                crate::infrastructure::persistence::postgres_config::PostgresHookConfigRepository::new(database_url)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to create database config repository: {}", e))?,
            );
            
            // 初始化数据库表
            repository.init_schema().await
                .map_err(|e| anyhow::anyhow!("Failed to initialize database schema: {}", e))?;
            
            let repository_clone = repository.clone();
            loaders.push(Arc::new(crate::infrastructure::config::DatabaseConfigLoader::new(
                repository_clone,
                config.tenant_id.clone(),
            )));
            
            Some(repository)
        } else {
            None
        };
        
        // 创建配置监听器
        let config_watcher = Arc::new(crate::infrastructure::config::ConfigWatcher::new(
            loaders,
            std::time::Duration::from_secs(config.refresh_interval_secs),
        ));
        
        // 启动配置监听
        config_watcher.start().await?;
        
        // 创建监控组件
        let metrics_collector = Arc::new(MetricsCollector::new());
        let execution_recorder = Arc::new(ExecutionRecorder::new());
        
        // 创建适配器工厂
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        
        // 创建编排服务
        let orchestration_service = Arc::new(HookOrchestrationService);
        
        // 创建应用服务
        let application_service = Arc::new(crate::application::service::HookApplicationService::new(
            orchestration_service,
            adapter_factory,
        ));
        
        // 创建命令和查询服务
        let command_service = Arc::new(HookCommandService::new(application_service.clone()));
        let query_service = Arc::new(HookQueryService::new(metrics_collector.clone()));
        
        // 创建Hook注册表
        let registry = Arc::new(crate::service::registry::CoreHookRegistry::new(
            config_watcher.clone(),
        ));
        
        Ok(Self {
            command_service,
            query_service,
            config_watcher,
            metrics_collector,
            execution_recorder,
            registry,
            application_service,
            config_repository,
        })
    }
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: HookEngineConfig) -> Result<()> {
        let engine = HookEngine::new(config).await?;
        
        // 启动gRPC服务器
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 50110));
        
        // HookExtension服务（Hook执行服务）
        let adapter_factory = Arc::new(HookAdapterFactory::new());
        let hook_extension_service = crate::interface::grpc::HookExtensionServer::new(
            engine.application_service.clone(),
            engine.registry.clone(),
            adapter_factory,
        );
        
        // HookService服务（Hook配置管理服务）
        let hook_management_service = if let Some(ref repository) = engine.config_repository {
            Some(crate::interface::grpc::HookServiceServer::new(
                repository.clone(),
                engine.registry.clone(),
            )
            .with_monitoring(
                engine.metrics_collector.clone(),
                engine.execution_recorder.clone(),
            ))
        } else {
            tracing::warn!("Database repository not available, HookService will not be available");
            None
        };
        
        tracing::info!("Starting Hook Engine gRPC server on {}", addr);
        
        // 创建shutdown信号
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let shutdown = async move {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install CTRL+C signal handler");
            tracing::info!("Shutting down Hook Engine gRPC server");
            let _ = tx.send(());
        };
        
        tokio::spawn(shutdown);
        
        // 构建服务器（使用链式调用）
        let mut server_builder = tonic::transport::Server::builder()
            .add_service(
                flare_proto::hooks::hook_extension_server::HookExtensionServer::new(hook_extension_service),
            );
        
        // 注册HookService服务（如果可用）
        if let Some(hook_service) = hook_management_service {
            server_builder = server_builder.add_service(
                flare_proto::hooks::hook_service_server::HookServiceServer::new(hook_service),
            );
            tracing::info!("HookService registered");
        }
        
        // 启动服务器
        tokio::select! {
            result = server_builder.serve(addr) => {
                if let Err(e) = result {
                    tracing::error!("gRPC server error: {}", e);
                    return Err(anyhow::anyhow!("gRPC server failed: {}", e));
                }
            }
            _ = rx => {
                tracing::info!("Received shutdown signal");
            }
        }
        
        Ok(())
    }
}

