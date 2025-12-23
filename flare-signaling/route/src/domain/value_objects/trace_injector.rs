//! Trace 注入器值对象
//!
//! 负责生成和注入 trace_id

use uuid::Uuid;
use crate::domain::service::RouteContext;

/// Trace 注入器
#[derive(Clone, Debug)]
pub struct TraceInjector;

impl TraceInjector {
    pub fn new() -> Self {
        Self
    }

    /// 注入 trace_id
    ///
    /// # 实现
    /// 当前使用 UUID，生产环境应使用分布式追踪系统（如 Jaeger、Zipkin）
    pub fn inject(&self, _ctx: &RouteContext) -> String {
        Uuid::new_v4().to_string()
    }
}

