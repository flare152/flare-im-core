//! # ConfigService Handler
//!
//! 提供系统级配置管理、服务发现、健康检查等能力。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待admin.proto生成Rust代码后启用
// use flare_proto::admin::config_service_server::ConfigService;
// use flare_proto::admin::{
//     GetServiceStatusRequest, GetServiceStatusResponse, GetSystemConfigRequest,
//     GetSystemConfigResponse, HealthCheckRequest, HealthCheckResponse,
//     ListServiceConfigsRequest, ListServiceConfigsResponse, ListServiceStatusesRequest,
//     ListServiceStatusesResponse, UpdateSystemConfigRequest, UpdateSystemConfigResponse,
// };
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::interface::interceptor::{extract_claims, extract_tenant_context};

/// ConfigService Handler实现
#[allow(dead_code)]
pub struct ConfigServiceHandler {
    // 配置中心客户端（TODO: 需要实现）
    // config_center_client: Arc<dyn ConfigCenterClient>,
}

impl ConfigServiceHandler {
    /// 创建ConfigService Handler
    pub fn new() -> Self {
        Self {
            // config_center_client,
        }
    }
}

// TODO: 等待admin.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl ConfigService for ConfigServiceHandler {
    /// 获取系统配置
    async fn get_system_config(
        &self,
        request: Request<GetSystemConfigRequest>,
    ) -> Result<Response<GetSystemConfigResponse>, Status> {
        let req = request.into_inner();
        debug!(config_key = %req.key, "GetSystemConfig request");

        // TODO: 从配置中心获取系统配置
        Ok(Response::new(GetSystemConfigResponse {
            config: None,
            status: None,
        }))
    }

    /// 更新系统配置
    async fn update_system_config(
        &self,
        request: Request<UpdateSystemConfigRequest>,
    ) -> Result<Response<UpdateSystemConfigResponse>, Status> {
        let req = request.into_inner();
        debug!(config_key = %req.key, "UpdateSystemConfig request");

        // TODO: 更新配置中心系统配置
        Ok(Response::new(UpdateSystemConfigResponse {
            status: None,
        }))
    }

    /// 列出服务配置
    async fn list_service_configs(
        &self,
        request: Request<ListServiceConfigsRequest>,
    ) -> Result<Response<ListServiceConfigsResponse>, Status> {
        let _req = request.into_inner();
        debug!("ListServiceConfigs request");

        // TODO: 列出所有服务配置
        Ok(Response::new(ListServiceConfigsResponse {
            configs: vec![],
            total: 0,
            status: None,
        }))
    }

    /// 健康检查
    async fn health_check(
        &self,
        request: Request<HealthCheckRequest>,
    ) -> Result<Response<HealthCheckResponse>, Status> {
        let _req = request.into_inner();
        debug!("HealthCheck request");

        // TODO: 检查服务健康状态
        Ok(Response::new(HealthCheckResponse {
            healthy: true,
            status: None,
        }))
    }

    /// 获取服务状态
    async fn get_service_status(
        &self,
        request: Request<GetServiceStatusRequest>,
    ) -> Result<Response<GetServiceStatusResponse>, Status> {
        let req = request.into_inner();
        debug!(service_name = %req.service_name, "GetServiceStatus request");

        // TODO: 查询服务状态
        Ok(Response::new(GetServiceStatusResponse {
            status: None,
        }))
    }

    /// 列出所有服务状态
    async fn list_service_statuses(
        &self,
        request: Request<ListServiceStatusesRequest>,
    ) -> Result<Response<ListServiceStatusesResponse>, Status> {
        let _req = request.into_inner();
        debug!("ListServiceStatuses request");

        // TODO: 列出所有服务状态
        Ok(Response::new(ListServiceStatusesResponse {
            services: vec![],
            total: 0,
            status: None,
        }))
    }
}
*/

