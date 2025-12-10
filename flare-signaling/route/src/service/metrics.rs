//! Router 指标（路由延迟、分片分布、流控拦截统计）

use prometheus::{
    HistogramVec, IntCounterVec, Opts, Registry,
};
use std::sync::Arc;

/// Router 指标集合
#[derive(Clone)]
pub struct RouterMetrics {
    /// 路由解析延迟（毫秒）
    pub route_resolve_duration_ms: HistogramVec,
    /// 分片分布（shard_id → 计数）
    pub shard_distribution: IntCounterVec,
    /// 流控拦截次数
    pub flow_control_blocked_total: IntCounterVec,
}

impl RouterMetrics {
    pub fn new(registry: Option<&Registry>) -> Arc<Self> {
        let route_resolve_duration_ms = HistogramVec::new(
            prometheus::HistogramOpts::new(
                "router_resolve_duration_ms",
                "Router endpoint resolution latency in milliseconds",
            )
            .buckets(vec![1.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0]),
            &["svid", "tenant_id"],
        )
        .expect("Failed to create router_resolve_duration_ms metric");

        let shard_distribution = IntCounterVec::new(
            Opts::new(
                "router_shard_distribution_total",
                "Total requests routed to each shard",
            ),
            &["shard_id", "svid"],
        )
        .expect("Failed to create router_shard_distribution_total metric");

        let flow_control_blocked_total = IntCounterVec::new(
            Opts::new(
                "router_flow_control_blocked_total",
                "Total requests blocked by flow control",
            ),
            &["reason", "svid"],
        )
        .expect("Failed to create router_flow_control_blocked_total metric");

        if let Some(reg) = registry {
            let _ = reg.register(Box::new(route_resolve_duration_ms.clone()));
            let _ = reg.register(Box::new(shard_distribution.clone()));
            let _ = reg.register(Box::new(flow_control_blocked_total.clone()));
        }

        Arc::new(Self {
            route_resolve_duration_ms,
            shard_distribution,
            flow_control_blocked_total,
        })
    }
}
