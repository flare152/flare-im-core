//! 消息路由领域服务
//!
//! 负责消息路由的核心业务逻辑：分片选择、负载均衡、流控、跨机房选择

use std::sync::Arc;
use anyhow::{Context as AnyhowContext, Result};
use chrono::Utc;

use crate::domain::model::Svid;
use crate::domain::repository::RouteRepository;
use crate::domain::value_objects::{
    ShardManager, ServiceLoadBalancer, FlowController, AzSelector, TraceInjector,
};
use crate::domain::service::RouteContext;

/// 消息路由领域服务
///
/// 职责：
/// - 分片选择（基于 conversation_id/user_id）
/// - 负载均衡（在同分片内选择实例）
/// - 流控检查（会话QPS、群聊fanout、系统反压）
/// - 跨机房选择（基于地理/负载/健康度）
/// - Trace 注入
pub struct MessageRoutingDomainService {
    shard_manager: ShardManager,
    service_lb: ServiceLoadBalancer,
    flow_controller: FlowController,
    az_selector: AzSelector,
    trace_injector: TraceInjector,
    route_repository: Arc<dyn RouteRepository>,
}

impl MessageRoutingDomainService {
    pub fn new(
        shard_count: usize,
        route_repository: Arc<dyn RouteRepository>,
    ) -> Self {
        Self {
            shard_manager: ShardManager::new(shard_count),
            service_lb: ServiceLoadBalancer::new(),
            flow_controller: FlowController::new(),
            az_selector: AzSelector::new(),
            trace_injector: TraceInjector::new(),
            route_repository,
        }
    }

    /// 解析端点（核心路由逻辑）
    ///
    /// # 流程
    /// 1. Trace 注入：生成 trace_id 并传播到下游
    /// 2. 流控检查：会话QPS + 群聊fanout + 系统反压
    /// 3. 分片选择：`shard_id = hash(conversation_id|user_id) % N`
    /// 4. 跨机房选择：基于地理/负载/健康度（可选）
    /// 5. 服务发现：从仓储获取路由信息
    /// 6. 业务负载均衡：在同 shard + 同 az 的实例内根据策略选择
    /// 7. 返回端点：gRPC 地址
    pub async fn resolve_endpoint(&self, ctx: &RouteContext) -> Result<String> {
        let start = Utc::now();
        
        // 1. Trace 注入
        let trace_id = self.trace_injector.inject(ctx);
        tracing::debug!(trace_id = %trace_id, svid = %ctx.svid, "Router resolving endpoint");

        // 2. 流控检查
        self.flow_controller.check(ctx).await
            .with_context(|| "Flow control check failed")?;

        // 3. 分片选择
        let shard = self
            .shard_manager
            .pick_shard(ctx.conversation_id.as_deref(), ctx.user_id.as_deref());

        // 4. 读取路由表
        let svid = Svid::new(ctx.svid.clone())
            .map_err(|e| anyhow::anyhow!("Invalid SVID: {}", e))?;
        
        let route = self.route_repository.find_by_svid(svid.as_str()).await
            .with_context(|| "Failed to read route repository")?;

        let candidate = route.map(|r| r.endpoint().as_str().to_string());
        let candidates = candidate.into_iter().collect::<Vec<_>>();

        // 5. 负载均衡选择候选
        let endpoint = self
            .service_lb
            .pick_service(svid.as_str(), shard, &candidates)
            .ok_or_else(|| anyhow::anyhow!("No endpoint candidates for SVID {}", svid))?;

        let elapsed_ms = (Utc::now() - start).num_milliseconds() as f64;

        tracing::info!(
            svid = %svid,
            shard = shard,
            endpoint = %endpoint,
            elapsed_ms = elapsed_ms,
            "✅ Router resolved endpoint"
        );

        Ok(endpoint)
    }
}

