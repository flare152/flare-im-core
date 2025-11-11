//! 应用启动器 - 负责依赖注入和服务启动
use anyhow::Result;
use std::sync::Arc;
use flare_im_core::FlareAppConfig;
use flare_server_core::ServiceRegistryTrait;
use crate::application::commands::MediaCommandService;
use crate::application::queries::MediaQueryService;
use crate::application::service::MediaApplication;
use crate::config::MediaConfig;
use crate::domain::repositories::{
    LocalStoreRef, MetadataCacheRef, MetadataStoreRef, ObjectRepositoryRef, ReferenceStoreRef,
    UploadSessionStoreRef,
};
use crate::domain::service::MediaService;
use crate::infrastructure::cache::redis_metadata::RedisMetadataCache;
use crate::infrastructure::local::filesystem::FilesystemMediaStore;
use crate::infrastructure::object_store::adapter::build_object_store;
use crate::infrastructure::persistence::postgres_metadata::PostgresMetadataStore;
use crate::infrastructure::session::redis_session::RedisUploadSessionStore;
use crate::interface::grpc::handler::MediaGrpcHandler;
use crate::interface::grpc::server::MediaGrpcServer;
use crate::service::registry::ServiceRegistrar;
use tonic::transport::Server;
use tracing::info;

/// 应用上下文 - 包含所有已初始化的服务
pub struct ApplicationContext {
    pub grpc_server: Arc<MediaGrpcServer>,
    pub service_registry: Option<std::sync::Arc<tokio::sync::RwLock<Box<dyn ServiceRegistryTrait>>>>,
    pub service_info: Option<flare_server_core::ServiceInfo>,
}

/// 应用启动器
pub struct ApplicationBootstrap;

impl ApplicationBootstrap {
    /// 运行应用的主入口点
    pub async fn run(config: &'static FlareAppConfig) -> Result<()> {
        // 创建应用上下文
        let context = Self::create_context(config).await?;
        
        // 获取运行时配置
        let service_cfg = config.media_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-media");
        
        // 启动服务器
        Self::start_server(
            context,
            &runtime_config.server.address,
            runtime_config.server.port,
        ).await
    }

    /// 创建应用上下文
    pub async fn create_context(config: &FlareAppConfig) -> Result<ApplicationContext> {
        let service_cfg = config.media_service();
        let runtime_config = config.compose_service_config(&service_cfg.runtime, "flare-media");
        let service_type = runtime_config.service.name.clone();
        let media_config = MediaConfig::from_app_config(config);
        
        // 注册服务
        let (service_registry, service_info) = ServiceRegistrar::register_service(&runtime_config, &service_type).await?;

        // 构建核心服务
        let media_service = Self::build_media_service(&media_config).await?;
        let application = Arc::new(MediaApplication::new(media_service));
        let command_service = Arc::new(MediaCommandService::new(application.clone()));
        let query_service = Arc::new(MediaQueryService::new(application));
        
        let handler = Arc::new(MediaGrpcHandler::new(command_service, query_service));
        let grpc_server = Arc::new(MediaGrpcServer::new(handler));

        Ok(ApplicationContext {
            grpc_server,
            service_registry,
            service_info,
        })
    }

    /// 构建媒体服务
    async fn build_media_service(config: &MediaConfig) -> Result<Arc<MediaService>> {
        let object_repo: Option<ObjectRepositoryRef> =
            build_object_store(config.object_store.as_ref()).await?;

        // 添加调试日志
        if let Some(ref object_store_config) = config.object_store {
            tracing::debug!(
                "Object store config: profile_type={}, endpoint={:?}, bucket={:?}",
                object_store_config.profile_type,
                object_store_config.endpoint,
                object_store_config.bucket
            );
        } else {
            tracing::debug!("No object store config found");
        }

        let (metadata_store, reference_store): (Option<MetadataStoreRef>, Option<ReferenceStoreRef>) =
            match config.postgres_url() {
                Some(url) => {
                    let store = PostgresMetadataStore::new(url).await?;
                    let metadata_store: MetadataStoreRef = Arc::new(store.clone());
                    let reference_store: ReferenceStoreRef = Arc::new(store.clone());
                    (Some(metadata_store), Some(reference_store))
                }
                None => (None, None),
            };

        let metadata_cache: Option<MetadataCacheRef> = match config.redis_url() {
            Some(url) => Some(
                Arc::new(RedisMetadataCache::new(url, &config.redis_namespace).await?)
                    as MetadataCacheRef,
            ),
            None => None,
        };

        let upload_session_store: Option<UploadSessionStoreRef> =
            match config.upload_session_redis_url() {
                Some(url) => {
                    let store = RedisUploadSessionStore::new(
                        url,
                        &config.upload_session_namespace,
                        config.chunk_ttl_seconds,
                    )
                    .await?;
                    Some(Arc::new(store) as UploadSessionStoreRef)
                }
                None => None,
            };

        let local_store: Option<LocalStoreRef> = match config.local_storage_dir.as_deref() {
            Some(dir) => Some(Arc::new(FilesystemMediaStore::new(
                dir,
                config.local_base_url.clone(),
            )?)),
            None => None,
        };

        Ok(Arc::new(MediaService::new(
            object_repo,
            metadata_store,
            reference_store,
            metadata_cache,
            upload_session_store,
            local_store,
            config.redis_ttl_seconds,
            config.cdn_base_url.clone(),
            config.orphan_grace_seconds,
            std::path::PathBuf::from(&config.chunk_upload_dir),
            config.chunk_ttl_seconds,
            config.max_chunk_size_bytes,
        )))
    }

    /// 启动gRPC服务器
    pub async fn start_server(
        context: ApplicationContext,
        address: &str,
        port: u16,
    ) -> Result<()> {
        let addr = format!("{}:{}", address, port).parse()?;
        info!(%addr, "starting media service");

        let server_future = Server::builder()
            .add_service(context.grpc_server.into_service())
            .serve(addr);

        // 等待服务器启动或接收到停止信号
        let result = tokio::select! {
            res = server_future => {
                if let Err(err) = res {
                    tracing::error!(error = %err, "media service failed");
                }
                Ok(())
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown signal received");
                Ok(())
            }
            else => {
                // 默认情况，服务器正常运行
                info!("media service running");
                Ok(())
            }
        };

        // 执行优雅停机
        Self::graceful_shutdown(context).await;

        info!("media service stopped");
        result
    }

    /// 优雅停机处理
    async fn graceful_shutdown(context: ApplicationContext) {
        // 如果有服务注册器，执行服务注销
        if let (Some(registry), Some(service_info)) = (&context.service_registry, &context.service_info) {
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