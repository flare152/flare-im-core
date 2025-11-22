//! # AccessGateway Handler
//!
//! 处理业务系统的推送请求，提供统一认证授权、租户上下文提取、代理转发功能。

use std::sync::Arc;

use anyhow::Result;
use flare_proto::access_gateway::access_gateway_server::AccessGateway;
use flare_proto::access_gateway::{
    BatchPushMessageRequest, BatchPushMessageResponse, ConnectionInfo, PushMessageRequest,
    PushMessageResponse, PushStatistics, QueryUserConnectionsRequest,
    QueryUserConnectionsResponse, UserConnectionInfo,
};
use tonic::{Request, Response, Status};
use tracing::{debug, info, instrument};

use crate::infrastructure::signaling::SignalingClient;
use crate::interface::interceptor::{extract_claims, extract_tenant_context};
use flare_im_core::gateway::GatewayRouterTrait;

/// Gateway Router trait（用于跨地区路由）
/// 使用 flare-im-core::gateway::GatewayRouterTrait
pub type GatewayRouter = dyn GatewayRouterTrait;

/// AccessGateway Handler实现
pub struct AccessGatewayHandler {
    /// Signaling客户端（用于查询在线状态）
    signaling_client: Arc<dyn SignalingClient>,
    /// Gateway Router（用于跨地区路由）
    gateway_router: Arc<GatewayRouter>,
}

impl AccessGatewayHandler {
    /// 创建AccessGateway Handler
    /// 
    /// # 类型参数
    /// * `S` - SignalingClient 实现类型
    /// * `R` - GatewayRouterTrait 实现类型
    pub fn new<S, R>(signaling_client: Arc<S>, gateway_router: Arc<R>) -> Self
    where
        S: SignalingClient + 'static,
        R: GatewayRouterTrait + 'static,
    {
        Self {
            signaling_client: signaling_client as Arc<dyn SignalingClient>,
            gateway_router: gateway_router as Arc<GatewayRouter>,
        }
    }
    
    /// 处理单个推送请求（辅助方法）
    async fn process_single_push(
        router: Arc<GatewayRouter>,
        online_status: flare_proto::signaling::GetOnlineStatusResponse,
        _tenant_context: flare_proto::TenantContext,
        push_req: PushMessageRequest,
    ) -> Result<PushMessageResponse> {
        // 按gateway_id分组用户
        let mut gateway_groups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        
        for user_id in &push_req.target_user_ids {
            if let Some(status) = online_status.statuses.get(user_id) {
                if status.online {
                    // 优先使用 gateway_id（从 Signaling Online 服务获取）
                    let gateway_id = if !status.gateway_id.is_empty() {
                        status.gateway_id.clone()
                    } else if !status.server_id.is_empty() {
                        // 兼容旧版本：如果没有 gateway_id，使用 server_id
                        status.server_id.clone()
                    } else {
                        // 最后兼容：使用 cluster_id
                        status.cluster_id.clone()
                    };
                    gateway_groups
                        .entry(gateway_id)
                        .or_insert_with(Vec::new)
                        .push(user_id.clone());
                }
            }
        }
        
        // 并发推送到各个Gateway
        let mut tasks = Vec::new();
        for (gateway_id, user_ids) in gateway_groups {
            let router_clone = Arc::clone(&router);
            let mut req = push_req.clone();
            req.target_user_ids = user_ids;
            
            tasks.push(tokio::spawn(async move {
                router_clone.route_push_message(&gateway_id, req).await
            }));
        }
        
        // 聚合结果
        let mut all_results = Vec::new();
        let mut total_users = 0;
        let mut online_users = 0;
        let mut offline_users = 0;
        let mut success_count = 0;
        let mut failure_count = 0;
        
        for task in tasks {
            match task.await {
                Ok(Ok(response)) => {
                    if let Some(stats) = &response.statistics {
                        total_users += stats.total_users;
                        online_users += stats.online_users;
                        offline_users += stats.offline_users;
                        success_count += stats.success_count;
                        failure_count += stats.failure_count;
                    }
                    all_results.extend(response.results);
                }
                Ok(Err(e)) => {
                    failure_count += 1;
                    debug!(error = %e, "Failed to push message to gateway");
                }
                Err(e) => {
                    failure_count += 1;
                    debug!(error = %e, "Task join error");
                }
            }
        }
        
        Ok(PushMessageResponse {
            results: all_results,
            status: Some(flare_server_core::error::ok_status()),
            statistics: Some(PushStatistics {
                total_users,
                online_users,
                offline_users,
                success_count,
                failure_count,
            }),
        })
    }
}

#[tonic::async_trait]
impl AccessGateway for AccessGatewayHandler {
    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        // 1. 先提取认证信息和租户上下文（在into_inner之前）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?
            .clone();
        
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?
            .clone();
        
        // 获取用户数量用于日志（在移动request之前）
        let user_count = request.get_ref().target_user_ids.len();
        
        // 2. 然后获取请求内容
        let req = request.into_inner();
        
        info!(
            tenant_id = %tenant_context.tenant_id,
            user_id = %claims.user_id,
            target_count = user_count,
            "PushMessage request from business system"
        );
        
        // 3. 查询用户在线状态（批量查询）
        let online_status = self
            .signaling_client
            .get_online_status(flare_proto::signaling::GetOnlineStatusRequest {
                user_ids: req.target_user_ids.clone(),
                context: Some(flare_proto::RequestContext::default()),
                tenant: Some(tenant_context.clone()),
            })
            .await
            .map_err(|e| {
                Status::internal(format!("Failed to query online status: {}", e))
            })?;
        
        // 4. 按server_id（作为gateway_id）分组用户
        let mut gateway_groups: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        
        for (user_id, status) in online_status.statuses {
            if status.online {
                // 使用server_id作为gateway_id（如果没有server_id，使用cluster_id）
                let gateway_id = if !status.server_id.is_empty() {
                    status.server_id
                } else {
                    status.cluster_id
                };
                gateway_groups
                    .entry(gateway_id)
                    .or_insert_with(Vec::new)
                    .push(user_id);
            }
        }
        
        // 5. 并发推送到各个Gateway
        let mut tasks = Vec::new();
        for (gateway_id, user_ids) in gateway_groups {
            let router = Arc::clone(&self.gateway_router);
            let mut push_req = req.clone();
            push_req.target_user_ids = user_ids;
            
            tasks.push(tokio::spawn(async move {
                router.route_push_message(&gateway_id, push_req).await
            }));
        }
        
        // 6. 等待所有推送完成并聚合结果
        let mut all_results = Vec::new();
        let mut total_users = 0;
        let mut online_users = 0;
        let mut offline_users = 0;
        let mut success_count = 0;
        let mut failure_count = 0;
        
        for task in tasks {
            match task.await {
                Ok(Ok(response)) => {
                    if let Some(stats) = &response.statistics {
                        total_users += stats.total_users;
                        online_users += stats.online_users;
                        offline_users += stats.offline_users;
                        success_count += stats.success_count;
                        failure_count += stats.failure_count;
                    }
                    all_results.extend(response.results);
                }
                Ok(Err(e)) => {
                    failure_count += 1;
                    debug!(error = %e, "Failed to push message to gateway");
                }
                Err(e) => {
                    failure_count += 1;
                    debug!(error = %e, "Task join error");
                }
            }
        }
        
        // 7. 构建响应
        let response = PushMessageResponse {
            results: all_results,
            status: Some(flare_server_core::error::ok_status()),
            statistics: Some(PushStatistics {
                total_users,
                online_users,
                offline_users,
                success_count,
                failure_count,
            }),
        };
        
        Ok(Response::new(response))
    }

    #[instrument(skip(self), fields(task_count = request.get_ref().pushes.len()))]
    async fn batch_push_message(
        &self,
        request: Request<BatchPushMessageRequest>,
    ) -> Result<Response<BatchPushMessageResponse>, Status> {
        // 1. 先提取认证和租户上下文（在移动request之前）
        let claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?
            .clone();
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?
            .clone();
        
        let task_count = request.get_ref().pushes.len();
        let req = request.into_inner();
        
        info!(
            tenant_id = %tenant_context.tenant_id,
            user_id = %claims.user_id,
            task_count = task_count,
            "BatchPushMessage request from business system"
        );
        
        // 2. 收集所有需要查询的用户ID
        let mut all_user_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for push in &req.pushes {
            all_user_ids.extend(push.target_user_ids.clone());
        }
        let all_user_ids_vec: Vec<String> = all_user_ids.into_iter().collect();
        
        // 3. 批量查询在线状态
        let online_status = self
            .signaling_client
            .get_online_status(flare_proto::signaling::GetOnlineStatusRequest {
                user_ids: all_user_ids_vec.clone(),
                context: Some(flare_proto::RequestContext::default()),
                tenant: Some(tenant_context.clone()),
            })
            .await
            .map_err(|e| {
                Status::internal(format!("Failed to query online status: {}", e))
            })?;
        
        // 4. 为每个推送任务处理
        let mut all_results = Vec::new();
        let mut total_tasks = 0;
        let mut success_tasks = 0;
        let mut failure_tasks = 0;
        let mut total_users = 0;
        let mut success_users = 0;
        let mut failure_users = 0;
        
        let options = req.options.unwrap_or_default();
        let max_concurrency = options.max_concurrency.max(1).min(1000) as usize;
        let parallel = options.parallel;
        
        if parallel {
            // 并行处理多个推送任务
            let mut tasks = Vec::new();
            for push_req in req.pushes {
                let router = Arc::clone(&self.gateway_router);
                let online_status_clone = online_status.clone();
                let tenant_context_clone = tenant_context.clone();
                
                tasks.push(tokio::spawn(async move {
                    Self::process_single_push(
                        router,
                        online_status_clone,
                        tenant_context_clone,
                        push_req,
                    ).await
                }));
            }
            
            // 限制并发数
            use futures::stream::{self, StreamExt};
            let mut stream = stream::iter(tasks)
                .buffer_unordered(max_concurrency);
            
            while let Some(task_result) = stream.next().await {
                total_tasks += 1;
                match task_result {
                    Ok(Ok(response)) => {
                        success_tasks += 1;
                        if let Some(stats) = &response.statistics {
                            total_users += stats.total_users;
                            success_users += stats.success_count;
                            failure_users += stats.failure_count;
                        }
                        all_results.push(response);
                        
                        if options.fail_fast && failure_users > 0 {
                            break;
                        }
                    }
                    Ok(Err(e)) => {
                        failure_tasks += 1;
                        debug!(error = %e, "Failed to process batch push task");
                        if options.fail_fast {
                            break;
                        }
                    }
                    Err(e) => {
                        failure_tasks += 1;
                        debug!(error = %e, "Task join error");
                        if options.fail_fast {
                            break;
                        }
                    }
                }
            }
        } else {
            // 串行处理
            for push_req in req.pushes {
                total_tasks += 1;
                match Self::process_single_push(
                    Arc::clone(&self.gateway_router),
                    online_status.clone(),
                    tenant_context.clone(),
                    push_req,
                ).await {
                    Ok(response) => {
                        success_tasks += 1;
                        if let Some(stats) = &response.statistics {
                            total_users += stats.total_users;
                            success_users += stats.success_count;
                            failure_users += stats.failure_count;
                        }
                        all_results.push(response);
                        
                        if options.fail_fast && failure_users > 0 {
                            break;
                        }
                    }
                    Err(e) => {
                        failure_tasks += 1;
                        debug!(error = %e, "Failed to process batch push task");
                        if options.fail_fast {
                            break;
                        }
                    }
                }
            }
        }
        
        // 5. 构建响应
        let response = BatchPushMessageResponse {
            results: all_results,
            status: Some(flare_server_core::error::ok_status()),
            statistics: Some(flare_proto::access_gateway::BatchPushStatistics {
                total_tasks,
                success_tasks,
                failure_tasks,
                total_users,
                success_users,
                failure_users,
            }),
        };
        
        Ok(Response::new(response))
    }

    async fn query_user_connections(
        &self,
        request: Request<QueryUserConnectionsRequest>,
    ) -> Result<Response<QueryUserConnectionsResponse>, Status> {
        // 先提取认证和租户上下文（在移动request之前）
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?
            .clone();
        
        // 获取用户数量用于日志（在移动request之前）
        let user_count = request.get_ref().user_ids.len();
        let req = request.into_inner();
        info!("QueryUserConnections request: {} users", user_count);
        
        // 查询在线状态
        let online_status = self
            .signaling_client
            .get_online_status(flare_proto::signaling::GetOnlineStatusRequest {
                user_ids: req.user_ids.clone(),
                context: Some(flare_proto::RequestContext::default()),
                tenant: Some(tenant_context.clone()),
            })
            .await
            .map_err(|e| {
                Status::internal(format!("Failed to query online status: {}", e))
            })?;
        
        // 转换为连接信息
        let users = online_status
            .statuses
            .into_iter()
            .map(|(user_id, status)| {
                let connections = if status.online {
                    vec![ConnectionInfo {
                        connection_id: format!("conn-{}", user_id), // TODO: 从实际连接获取
                        protocol: "websocket".to_string(),
                        device_id: status.device_id,
                        platform: status.device_platform,
                        connected_at: status.last_seen.clone(),
                        last_active_at: status.last_seen,
                    }]
                } else {
                    Vec::new()
                };

                UserConnectionInfo {
                    user_id,
                    online: status.online,
                    connections,
                    last_active_at: status.last_seen,
                }
            })
            .collect();
        
        Ok(Response::new(QueryUserConnectionsResponse {
            users,
            status: Some(flare_server_core::error::ok_status()),
        }))
    }
}

