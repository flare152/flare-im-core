//! # HookService Handler
//!
//! 提供Hook配置管理能力，支持Hook配置的CRUD和查询。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待admin.proto生成Rust代码后启用
// use flare_proto::admin::hook_service_server::HookService;
// use flare_proto::admin::{
//     CreateHookConfigRequest, CreateHookConfigResponse, DeleteHookConfigRequest,
//     DeleteHookConfigResponse, GetHookConfigRequest, GetHookConfigResponse,
//     GetHookStatisticsRequest, GetHookStatisticsResponse, ListHookConfigsRequest,
//     ListHookConfigsResponse, QueryHookErrorsRequest, QueryHookErrorsResponse,
//     QueryHookExecutionsRequest, QueryHookExecutionsResponse, SetHookStatusRequest,
//     SetHookStatusResponse, UpdateHookConfigRequest, UpdateHookConfigResponse,
// };
use crate::infrastructure::hook_engine::HookEngineClientWrapper;

/// HookService Handler实现
/// 
/// **架构说明**：Gateway只做代理转发，不包含业务逻辑
/// - 前置校验：认证、授权、限流
/// - 代理转发：将请求转发到Hook Engine
pub struct HookServiceHandler {
    hook_engine_client: Arc<HookEngineClientWrapper>,
}

impl HookServiceHandler {
    /// 创建HookService Handler
    pub fn new(hook_engine_endpoint: String) -> Self {
        Self {
            hook_engine_client: Arc::new(HookEngineClientWrapper::new(hook_engine_endpoint)),
        }
    }
    
}

// TODO: 等待admin.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl HookService for HookServiceHandler {
    /// 创建Hook配置
    /// 
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn create_hook_config(
        &self,
        request: Request<CreateHookConfigRequest>,
    ) -> Result<Response<CreateHookConfigResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:create")?;
        
        // TODO: 限流检查
        // self.rate_limiter.check(&claims.user_id, "hook:create").await?;
        
        info!(
            user_id = %claims.user_id,
            "CreateHookConfig request (proxying to Hook Engine)"
        );
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.create_hook_config(request).await
    }

    /// 获取Hook配置
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn get_hook_config(
        &self,
        request: Request<GetHookConfigRequest>,
    ) -> Result<Response<GetHookConfigResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:read")?;
        
        debug!(user_id = %claims.user_id, "GetHookConfig request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.get_hook_config(request).await
    }

    /// 更新Hook配置
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn update_hook_config(
        &self,
        request: Request<UpdateHookConfigRequest>,
    ) -> Result<Response<UpdateHookConfigResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:update")?;
        
        debug!(user_id = %claims.user_id, "UpdateHookConfig request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.update_hook_config(request).await
    }

    /// 删除Hook配置
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn delete_hook_config(
        &self,
        request: Request<DeleteHookConfigRequest>,
    ) -> Result<Response<DeleteHookConfigResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:delete")?;
        
        debug!(user_id = %claims.user_id, "DeleteHookConfig request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.delete_hook_config(request).await
    }

    /// 列出Hook配置
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn list_hook_configs(
        &self,
        request: Request<ListHookConfigsRequest>,
    ) -> Result<Response<ListHookConfigsResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:read")?;
        
        debug!(user_id = %claims.user_id, "ListHookConfigs request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.list_hook_configs(request).await
    }

    /// 启用/禁用Hook配置
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn set_hook_status(
        &self,
        request: Request<SetHookStatusRequest>,
    ) -> Result<Response<SetHookStatusResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:update")?;
        
        debug!(user_id = %claims.user_id, "SetHookStatus request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.set_hook_status(request).await
    }

    /// 获取Hook统计信息
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn get_hook_statistics(
        &self,
        request: Request<GetHookStatisticsRequest>,
    ) -> Result<Response<GetHookStatisticsResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:read")?;
        
        debug!(user_id = %claims.user_id, "GetHookStatistics request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.get_hook_statistics(request).await
    }

    /// 查询Hook执行历史
    ///
    /// **架构说明**：Gateway只做前置校验和代理转发
    async fn query_hook_executions(
        &self,
        request: Request<QueryHookExecutionsRequest>,
    ) -> Result<Response<QueryHookExecutionsResponse>, Status> {
        // 1. 前置校验（认证、授权、限流）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        
        // TODO: 权限校验
        // check_permission(&claims, "hook:read")?;
        
        debug!(user_id = %claims.user_id, "QueryHookExecutions request (proxying to Hook Engine)");
        
        // 2. 代理转发到Hook Engine
        let mut client = self.hook_engine_client.create_client().await
            .map_err(|e| Status::internal(format!("Failed to create Hook Engine client: {}", e)))?;
        
        client.query_hook_executions(request).await
    }
}
*/
