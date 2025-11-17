//! 应用启动器 - 负责依赖注入和服务启动
use crate::application::commands::MessageCommandService;
use crate::config::MessageOrchestratorConfig;
use crate::domain::repositories::WalRepository;
use crate::infrastructure::messaging::kafka_publisher::KafkaMessagePublisher;
use crate::infrastructure::persistence::noop_wal::NoopWalRepository;
use crate::infrastructure::persistence::redis_wal::RedisWalRepository;
use crate::interface::grpc::handler::MessageGrpcHandler;
use crate::interface::grpc::server::MessageGrpcServer;
use crate::service::registry::ServiceRegistrar;
use anyhow::{Result, anyhow};
use flare_im_core::hooks::adapters::DefaultHookFactory;
use flare_im_core::hooks::{HookConfigLoader, HookDispatcher, HookRegistry};
use flare_im_core::metrics::MessageOrchestratorMetrics;
use flare_proto::storage::storage_reader_service_client::StorageReaderServiceClient;
use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use std::sync::Arc;
use tonic::transport::Server;
use tracing::{error, info};

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub grpc_server: Arc<MessageGrpcServer>,
    pub service_registry:
        Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn flare_server_core::ServiceRegistryTrait>>>>,
    pub service_info: Option<flare_server_core::ServiceInfo>,
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: &'static flare_im_core::config::FlareAppConfig) -> Result<()> {
        // 初始化 OpenTelemetry 追踪
        #[cfg(feature = "tracing")]
        {
            let otlp_endpoint = std::env::var("OTLP_ENDPOINT").ok();
            if let Err(e) = flare_im_core::tracing::init_tracing("message-orchestrator", otlp_endpoint.as_deref()) {
                error!(error = %e, "Failed to initialize OpenTelemetry tracing");
            } else {
                info!("✅ OpenTelemetry tracing initialized");
            }
        }

        // 创建应用上下文
        let context = Self::create_context(config).await?;

        // 获取运行时配置
        let service_cfg = config.message_orchestrator_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "message-orchestrator");

        // 启动服务器
        Self::start_server(
            context,
            &runtime_config.server.address,
            runtime_config.server.port,
        )
        .await
    }

    /// 创建应用上下文
    pub async fn create_context(
        config: &flare_im_core::config::FlareAppConfig,
    ) -> Result<ApplicationContext> {
        let service_cfg = config.message_orchestrator_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "message-orchestrator");
        let service_type = runtime_config.service.name.clone();
        let orchestrator_config = MessageOrchestratorConfig::from_app_config(config);

        // 注册服务
        let (service_registry, service_info) =
            ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        let command_service = Self::build_command_service(Arc::new(orchestrator_config)).await?;

        let handler = Arc::new(MessageGrpcHandler::new(command_service));
        let grpc_server = Arc::new(MessageGrpcServer::new(handler));

        Ok(ApplicationContext {
            grpc_server,
            service_registry,
            service_info,
        })
    }

    /// 构建命令服务
    async fn build_command_service(
        config: Arc<MessageOrchestratorConfig>,
    ) -> Result<Arc<MessageCommandService>> {
        // 创建 Kafka Producer
        // 优化配置以解决超时问题：
        // 1. 增加所有超时时间
        // 2. 设置重试配置
        // 3. 优化 metadata 获取
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("client.id", "message-orchestrator-producer")
            .set("message.timeout.ms", &config.kafka_timeout_ms.to_string())
            .set("socket.timeout.ms", "60000")
            .set("request.timeout.ms", "60000")
            .set("delivery.timeout.ms", "180000") // 增加到 3 分钟
            .set("metadata.request.timeout.ms", "60000") // 增加到 60 秒
            .set("api.version.request.timeout.ms", "60000") // 增加到 60 秒
            .set("retries", "3") // 重试 3 次
            .set("retry.backoff.ms", "1000") // 重试间隔 1 秒
            .set("max.in.flight.requests.per.connection", "5") // 允许最多 5 个未确认请求
            .set("acks", "1") // Leader 确认即可（平衡性能和可靠性）
            .set("compression.type", "none") // 禁用压缩以提高性能
            .set("broker.address.family", "v4") // 强制 IPv4
            .set("client.dns.lookup", "use_all_dns_ips") // DNS 解析策略
            .create()?;
        let producer = Arc::new(producer);

        let publisher = Arc::new(KafkaMessagePublisher::new(producer, config.clone()));
        let wal_repository = Self::build_wal_repository(&config)?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            storage_topic = %config.kafka_storage_topic,
            push_topic = %config.kafka_push_topic,
            "MessageOrchestrator connected to Kafka"
        );

        // 初始化 Hook 系统
        let hooks = Self::build_hook_dispatcher(&config).await?;

        // 初始化指标收集
        let metrics = Arc::new(MessageOrchestratorMetrics::new());

        // 创建 StoreMessageCommandHandler
        use crate::application::commands::StoreMessageCommandHandler;
        let store_handler = Arc::new(
            StoreMessageCommandHandler::new(
                publisher,
                wal_repository,
                config.defaults(),
                hooks,
                metrics,
            ),
        );

        // 初始化 Storage Reader 客户端（如果配置了 reader_endpoint）
        let reader_client = if let Some(endpoint) = &config.reader_endpoint {
            match tonic::transport::Endpoint::from_shared(endpoint.clone()) {
                Ok(endpoint) => {
                    match StorageReaderServiceClient::connect(endpoint.clone()).await {
                        Ok(client) => {
                            info!(endpoint = %endpoint.uri(), "Connected to Storage Reader");
                            Some(client)
                        }
                        Err(err) => {
                            error!(error = ?err, endpoint = %endpoint.uri(), "Failed to connect to Storage Reader");
                            None
                        }
                    }
                }
                Err(err) => {
                    error!(error = ?err, endpoint = %endpoint, "Invalid Storage Reader endpoint");
                    None
                }
            }
        } else {
            None
        };

        Ok(Arc::new(MessageCommandService::new(
            store_handler,
            reader_client,
        )))
    }

    /// 构建 Hook Dispatcher
    async fn build_hook_dispatcher(
        config: &MessageOrchestratorConfig,
    ) -> Result<Arc<HookDispatcher>> {
        let mut hook_loader = HookConfigLoader::new();
        if let Some(path) = &config.hook_config {
            hook_loader = hook_loader.add_candidate(path.clone());
        }
        if let Some(dir) = &config.hook_config_dir {
            hook_loader = hook_loader.add_candidate(dir.clone());
        }
        let hook_config = hook_loader.load().map_err(|err| anyhow!(err.to_string()))?;
        let registry = HookRegistry::builder().build();
        let hook_factory = DefaultHookFactory::new().map_err(|err| anyhow!(err.to_string()))?;
        hook_config
            .install(Arc::clone(&registry), &hook_factory)
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        Ok(Arc::new(HookDispatcher::new(registry)))
    }

    /// 构建 WAL Repository
    fn build_wal_repository(
        config: &Arc<MessageOrchestratorConfig>,
    ) -> Result<Arc<dyn WalRepository + Send + Sync>> {
        if let Some(url) = &config.redis_url {
            let client = Arc::new(redis::Client::open(url.as_str())?);
            let repo: Arc<dyn WalRepository + Send + Sync> =
                Arc::new(RedisWalRepository::new(client, config.clone()));
            Ok(repo)
        } else {
            Ok(NoopWalRepository::shared())
        }
    }

    /// 启动gRPC服务器
    pub async fn start_server(
        context: ApplicationContext,
        address: &str,
        port: u16,
    ) -> Result<()> {
        let addr = format!("{}:{}", address, port).parse()?;
        info!(%addr, "starting message orchestrator service");

        let server_future = Server::builder()
            .add_service(context.grpc_server.into_service())
            .serve(addr);

        // 等待服务器启动或接收到停止信号
        let result = tokio::select! {
            res = server_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "message orchestrator service failed");
                }
                Ok(())
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                Ok(())
            }
        };

        // 执行优雅停机
        Self::graceful_shutdown(context).await;

        info!("message orchestrator service stopped");
        result
    }

    /// 优雅停机处理
    async fn graceful_shutdown(context: ApplicationContext) {
        // 如果有服务注册器，执行服务注销
        if let (Some(registry), Some(service_info)) =
            (&context.service_registry, &context.service_info)
        {
            info!("unregistering service...");
            let mut registry = registry.write().await;
            if let Err(e) = registry.unregister(&service_info.instance_id).await {
                tracing::warn!(error = %e, "failed to unregister service");
            } else {
                info!("service unregistered successfully");
            }
        }

        // 在这里可以添加其他需要优雅停机的资源清理操作
        // 例如：关闭数据库连接、清理临时文件等
    }
}

