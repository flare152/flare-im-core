//! 服务注册器 - 负责服务的注册和发现
use anyhow::Result;
use flare_server_core::{ServiceInfo, ServiceRegistryTrait, create_registry};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// 服务注册器
pub struct ServiceRegistrar;

impl ServiceRegistrar {
    /// 注册服务
    pub async fn register_service(
        runtime_config: &flare_server_core::Config,
        service_type: &str,
    ) -> Result<(
        Option<Arc<RwLock<Box<dyn ServiceRegistryTrait>>>>,
        Option<ServiceInfo>,
    )> {
        if let Some(reg_config) = &runtime_config.registry {
            match create_registry(reg_config.clone()).await {
                Ok(mut registry) => {
                    let service_info = ServiceInfo {
                        service_type: service_type.to_string(),
                        service_id: runtime_config.service.name.clone(),
                        instance_id: uuid::Uuid::new_v4().to_string(),
                        address: runtime_config.server.address.clone(),
                        port: runtime_config.server.port,
                        metadata: std::collections::HashMap::new(),
                    };

                    if let Err(e) = registry.register(service_info.clone()).await {
                        tracing::warn!(error = %e, "failed to register service");
                        return Ok((None, Some(service_info)));
                    }

                    info!(
                        "Service registered: {} at {}:{}",
                        service_type, service_info.address, service_info.port
                    );

                    Ok((Some(Arc::new(RwLock::new(registry))), Some(service_info)))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "failed to create service registry");
                    Ok((None, None))
                }
            }
        } else {
            info!("Service registry not configured, skipping registration");
            Ok((None, None))
        }
    }
}
