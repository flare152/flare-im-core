//! 消息路由处理器
//!
//! 负责消息路由的业务流程编排

use std::sync::Arc;
use std::time::Instant;
use flare_proto::signaling::router::RouteOptions;
use flare_server_core::context::{Context, ContextExt};
use flare_server_core::error::ErrorCode;
use tracing::instrument;

use crate::infrastructure::forwarder::MessageForwarder;
use crate::application::dto::{MessageRouteResult, build_route_metadata};

/// 消息路由处理器
///
/// 职责：
/// - 编排消息路由流程
/// - 调用领域服务进行路由决策
/// - 调用基础设施层进行消息转发
pub struct MessageRoutingHandler {
    message_forwarder: Arc<MessageForwarder>,
}

impl MessageRoutingHandler {
    pub fn new(
        message_forwarder: Arc<MessageForwarder>,
    ) -> Self {
        Self {
            message_forwarder,
        }
    }

    /// 路由消息到业务系统
    ///
    /// # 流程
    /// 1. 提取路由选项和追踪上下文
    /// 2. 调用消息转发服务转发消息
    /// 3. 构建路由元数据
    ///
    /// # 返回
    /// 消息路由结果（包含响应数据、端点地址和路由元数据）
    #[instrument(skip(self, ctx), fields(
        request_id = %ctx.request_id(),
        trace_id = %ctx.trace_id(),
        svid = %svid,
    ))]
    pub async fn route_message(
        &self,
        ctx: &Context,
        svid: &str,
        payload: Vec<u8>,
        route_options: RouteOptions,
    ) -> MessageRouteResult {
        ctx.ensure_not_cancelled().map_err(|e| {
            flare_server_core::error::ErrorBuilder::new(
                ErrorCode::InternalError,
                "Request cancelled",
            )
            .details(e.to_string())
            .build_error()
        }).ok(); // 忽略取消错误，继续处理
        let start_time = Instant::now();
        let decision_start = Instant::now();
        
        // 提取追踪上下文
        let trace_context: Option<flare_proto::common::TraceContext> = if route_options.enable_tracing {
            ctx.request()
                .and_then(|req| req.trace.clone().map(|tc| tc.into()))
                .or_else(|| {
                    if !ctx.trace_id().is_empty() {
                        Some(flare_proto::common::TraceContext {
                            trace_id: ctx.trace_id().to_string(),
                            span_id: String::new(),
                            parent_span_id: String::new(),
                            sampled: "yes".to_string(),
                            tags: std::collections::HashMap::new(),
                        })
                    } else {
                        None
                    }
                })
        } else {
            None
        };
        
        // 记录路由决策耗时
        let decision_duration = decision_start.elapsed();
        
        // 调用消息转发服务转发消息
        let business_start = Instant::now();
        match self.message_forwarder
            .forward_message(ctx, svid, payload, None)
            .await
        {
            Ok((endpoint, response_data)) => {
                let business_duration = business_start.elapsed();
                let total_duration = start_time.elapsed();
                
                tracing::info!(
                    svid = %svid,
                    routed_endpoint = %endpoint,
                    response_len = %response_data.len(),
                    decision_duration_ms = decision_duration.as_millis(),
                    business_duration_ms = business_duration.as_millis(),
                    total_duration_ms = total_duration.as_millis(),
                    "Message routed successfully"
                );
                
                MessageRouteResult {
                    response_data,
                    routed_endpoint: endpoint,
                    metadata: build_route_metadata(
                        total_duration.as_millis() as i64,
                        business_duration.as_millis() as i64,
                        decision_duration.as_millis() as i64,
                        svid,
                        route_options.load_balance_strategy,
                        trace_context,
                    ),
                    error_code: None,
                    error_message: None,
                }
            }
            Err(e) => {
                let total_duration = start_time.elapsed();
                
                tracing::error!(
                    error = %e,
                    svid = %svid,
                    decision_duration_ms = decision_duration.as_millis(),
                    total_duration_ms = total_duration.as_millis(),
                    "Failed to forward message to business system"
                );
                
                MessageRouteResult {
                    response_data: vec![],
                    routed_endpoint: String::new(),
                    metadata: build_route_metadata(
                        total_duration.as_millis() as i64,
                        0,
                        decision_duration.as_millis() as i64,
                        svid,
                        route_options.load_balance_strategy,
                        trace_context,
                    ),
                    error_code: Some(ErrorCode::InternalError as u32),
                    error_message: Some(format!("Failed to forward message: {}", e)),
                }
            }
        }
    }
}

