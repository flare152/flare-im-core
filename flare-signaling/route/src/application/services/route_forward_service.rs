//! 路由转发服务 - 将消息转发到目标服务

use std::sync::Arc;

use flare_proto::signaling::RouteMessageRequest;
use flare_server_core::error::{ErrorCode, Result};
use tracing::error;

use crate::application::RouteDirectoryService;
use crate::util;

/// 路由转发服务
/// 负责将消息转发到目标业务服务
pub struct RouteForwardService {
    route_service: Arc<RouteDirectoryService>,
}

impl RouteForwardService {
    pub fn new(route_service: Arc<RouteDirectoryService>) -> Self {
        Self { route_service }
    }

    /// 转发消息到目标服务
    /// 
    /// 当前实现：
    /// - Lookup: 查找服务端点并返回
    /// - Register: 注册服务端点
    /// - Forward: 暂不支持（需要下游服务实现）
    pub async fn forward_message(&self, request: RouteMessageRequest) -> Result<flare_proto::signaling::RouteMessageResponse> {
        // 使用 RouteDirectoryService 的路由功能
        self.route_service.route(request).await
    }

    /// 批量转发消息
    pub async fn forward_messages(
        &self,
        requests: Vec<RouteMessageRequest>,
    ) -> Result<Vec<flare_proto::signaling::RouteMessageResponse>> {
        let mut results = Vec::new();
        
        for request in requests {
            match self.forward_message(request).await {
                Ok(response) => results.push(response),
                Err(err) => {
                    error!(error = %err, "failed to forward message");
                    results.push(flare_proto::signaling::RouteMessageResponse {
                        success: false,
                        response: vec![],
                        error_message: err.to_string(),
                        status: util::rpc_status_error(
                            ErrorCode::InternalError,
                            &format!("forward failed: {}", err),
                        ),
                    });
                }
            }
        }

        Ok(results)
    }
}

