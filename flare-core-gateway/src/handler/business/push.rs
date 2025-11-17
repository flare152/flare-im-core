//! # PushService Handler
//!
//! 提供推送通知发送、模板管理等能力，委托给Push Service。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待business.proto生成Rust代码后启用
// use flare_proto::business::push_service_server::PushService;
// use flare_proto::business::{
//     BatchSendPushRequest, BatchSendPushResponse, GetPushStatusRequest, GetPushStatusResponse,
//     QueryPushHistoryRequest, QueryPushHistoryResponse, SendPushRequest, SendPushResponse,
//     SendTemplatePushRequest, SendTemplatePushResponse,
// };
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::infrastructure::push::PushClient;
use crate::interceptor::{extract_claims, extract_tenant_context};

/// PushService Handler实现
pub struct PushServiceHandler {
    /// Push客户端
    push_client: Arc<dyn PushClient>,
}

impl PushServiceHandler {
    /// 创建PushService Handler
    pub fn new(push_client: Arc<dyn PushClient>) -> Self {
        Self { push_client }
    }
}

// TODO: 等待business.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl PushService for PushServiceHandler {
    /// 发送推送
    async fn send_push(
        &self,
        request: Request<SendPushRequest>,
    ) -> Result<Response<SendPushResponse>, Status> {
        let _req = request.into_inner();
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        let _tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?;

        debug!("SendPush request");

        // TODO: 委托给Push Service
        // 1. 构建Push请求
        // 2. 调用Push Service发送推送
        Err(Status::unimplemented("SendPush not yet implemented"))
    }

    /// 批量发送推送
    async fn batch_send_push(
        &self,
        request: Request<BatchSendPushRequest>,
    ) -> Result<Response<BatchSendPushResponse>, Status> {
        let _req = request.into_inner();
        debug!("BatchSendPush request");

        // TODO: 委托给Push Service批量发送
        Err(Status::unimplemented("BatchSendPush not yet implemented"))
    }

    /// 使用模板发送推送
    async fn send_template_push(
        &self,
        request: Request<SendTemplatePushRequest>,
    ) -> Result<Response<SendTemplatePushResponse>, Status> {
        let _req = request.into_inner();
        debug!("SendTemplatePush request");

        // TODO: 委托给Push Service使用模板发送
        Err(Status::unimplemented("SendTemplatePush not yet implemented"))
    }

    /// 获取推送状态
    async fn get_push_status(
        &self,
        request: Request<GetPushStatusRequest>,
    ) -> Result<Response<GetPushStatusResponse>, Status> {
        let _req = request.into_inner();
        debug!("GetPushStatus request");

        // TODO: 委托给Push Service查询状态
        Err(Status::unimplemented("GetPushStatus not yet implemented"))
    }

    /// 查询推送历史
    async fn query_push_history(
        &self,
        request: Request<QueryPushHistoryRequest>,
    ) -> Result<Response<QueryPushHistoryResponse>, Status> {
        let _req = request.into_inner();
        debug!("QueryPushHistory request");

        // TODO: 委托给Push Service查询历史
        Err(Status::unimplemented("QueryPushHistory not yet implemented"))
    }
}
*/

