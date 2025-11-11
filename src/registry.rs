//! 服务注册发现模块

use crate::error::{ErrorCode, Result, map_infra_error};
use flare_server_core::{ServiceInfo, ServiceRegistryTrait, create_registry};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// 注册服务到注册中心
///
/// 返回注册表实例（如果配置了注册发现）
pub async fn register_service(
    config: &flare_server_core::Config,
    service_type: &str,
) -> Result<Option<Arc<RwLock<Box<dyn ServiceRegistryTrait>>>>> {
    if let Some(reg_config) = &config.registry {
        let mut registry = create_registry(reg_config.clone()).await.map_err(|err| {
            map_infra_error(
                err,
                ErrorCode::ServiceUnavailable,
                "failed to create service registry",
            )
        })?;

        let service_info = ServiceInfo {
            service_type: service_type.to_string(),
            service_id: config.service.name.clone(),
            instance_id: uuid::Uuid::new_v4().to_string(),
            address: config.server.address.clone(),
            port: config.server.port,
            metadata: std::collections::HashMap::new(),
        };

        registry
            .register(service_info.clone())
            .await
            .map_err(|err| {
                map_infra_error(
                    err,
                    ErrorCode::ServiceUnavailable,
                    "failed to register service instance",
                )
            })?;

        info!(
            "Service registered: {} at {}:{}",
            service_type, service_info.address, service_info.port
        );

        Ok(Some(Arc::new(RwLock::new(registry))))
    } else {
        info!("Service registry not configured, skipping registration");
        Ok(None)
    }
}
