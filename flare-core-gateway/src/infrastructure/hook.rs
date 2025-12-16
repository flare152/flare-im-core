//! Hook服务客户端

use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use flare_proto::hooks::hook_service_client::HookServiceClient;
use flare_proto::hooks::*;

use flare_server_core::discovery::ServiceClient;

/// gRPC Hook服务客户端
pub struct GrpcHookClient {
    /// 服务客户端（用于服务发现）
    service_client: Option<Arc<Mutex<ServiceClient>>>,
    /// 服务名称
    service_name: String,
    /// 直连地址（当没有服务发现时使用）
    direct_address: Option<String>,
}

impl GrpcHookClient {
    /// 创建新的gRPC Hook服务客户端
    pub fn new(service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: None,
        }
    }

    /// 使用服务客户端创建gRPC Hook服务客户端
    pub fn with_service_client(service_client: ServiceClient, service_name: String) -> Self {
        Self {
            service_client: Some(Arc::new(Mutex::new(service_client))),
            service_name,
            direct_address: None,
        }
    }

    /// 使用直接地址创建gRPC Hook服务客户端
    pub fn with_direct_address(direct_address: String, service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: Some(direct_address),
        }
    }

    /// 获取gRPC客户端
    async fn get_client(&self) -> Result<HookServiceClient<Channel>, Status> {
        if let Some(service_client) = &self.service_client {
            let mut client = service_client.lock().await;
            let channel = client.get_channel().await.map_err(|e| {
                Status::unavailable(format!(
                    "Failed to get channel from service discovery: {}",
                    e
                ))
            })?;
            Ok(HookServiceClient::new(channel))
        } else if let Some(ref address) = self.direct_address {
            let channel = Channel::from_shared(address.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| {
                    Status::unavailable(format!("Failed to connect to {}: {}", address, e))
                })?;
            Ok(HookServiceClient::new(channel))
        } else {
            // 使用服务名称进行直连（假设服务名称可以直接解析）
            let channel = Channel::from_shared(self.service_name.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid service name: {}", e)))?
                .connect()
                .await
                .map_err(|e| {
                    Status::unavailable(format!(
                        "Failed to connect to {}: {}",
                        self.service_name, e
                    ))
                })?;
            Ok(HookServiceClient::new(channel))
        }
    }

    /// 创建Hook配置
    pub async fn create_hook_config(
        &self,
        request: Request<CreateHookConfigRequest>,
    ) -> Result<Response<CreateHookConfigResponse>, Status> {
        let mut client = self.get_client().await?;
        client.create_hook_config(request).await
    }

    /// 获取Hook配置
    pub async fn get_hook_config(
        &self,
        request: Request<GetHookConfigRequest>,
    ) -> Result<Response<GetHookConfigResponse>, Status> {
        let mut client = self.get_client().await?;
        client.get_hook_config(request).await
    }

    /// 更新Hook配置
    pub async fn update_hook_config(
        &self,
        request: Request<UpdateHookConfigRequest>,
    ) -> Result<Response<UpdateHookConfigResponse>, Status> {
        let mut client = self.get_client().await?;
        client.update_hook_config(request).await
    }

    /// 列出Hook配置
    pub async fn list_hook_configs(
        &self,
        request: Request<ListHookConfigsRequest>,
    ) -> Result<Response<ListHookConfigsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.list_hook_configs(request).await
    }

    /// 删除Hook配置
    pub async fn delete_hook_config(
        &self,
        request: Request<DeleteHookConfigRequest>,
    ) -> Result<Response<DeleteHookConfigResponse>, Status> {
        let mut client = self.get_client().await?;
        client.delete_hook_config(request).await
    }

    /// 启用/禁用Hook
    pub async fn set_hook_status(
        &self,
        request: Request<SetHookStatusRequest>,
    ) -> Result<Response<SetHookStatusResponse>, Status> {
        let mut client = self.get_client().await?;
        client.set_hook_status(request).await
    }

    /// 查询Hook执行统计
    pub async fn get_hook_statistics(
        &self,
        request: Request<GetHookStatisticsRequest>,
    ) -> Result<Response<GetHookStatisticsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.get_hook_statistics(request).await
    }

    /// 查询Hook执行历史
    pub async fn query_hook_executions(
        &self,
        request: Request<QueryHookExecutionsRequest>,
    ) -> Result<Response<QueryHookExecutionsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.query_hook_executions(request).await
    }
}
