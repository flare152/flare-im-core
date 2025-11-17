//! # UserService Handler
//!
//! 提供用户在线状态、设备管理等能力，委托给Signaling Service。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待business.proto生成Rust代码后启用
// use flare_proto::business::user_service_server::UserService;
// use flare_proto::business::{
//     BatchGetUserPresenceRequest, BatchGetUserPresenceResponse, GetDeviceRequest,
//     GetDeviceResponse, GetUserPresenceRequest, GetUserPresenceResponse,
//     KickDeviceRequest, KickDeviceResponse, ListUserDevicesRequest, ListUserDevicesResponse,
//     SubscribeUserPresenceRequest, SubscribeUserPresenceResponse,
// };
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::infrastructure::signaling::SignalingClient;
use crate::interceptor::{extract_claims, extract_tenant_context};

/// UserService Handler实现
pub struct UserServiceHandler {
    /// Signaling客户端
    signaling_client: Arc<dyn SignalingClient>,
}

impl UserServiceHandler {
    /// 创建UserService Handler
    pub fn new(signaling_client: Arc<dyn SignalingClient>) -> Self {
        Self { signaling_client }
    }
}

// TODO: 等待business.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl UserService for UserServiceHandler {
    /// 获取用户在线状态
    async fn get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> Result<Response<GetUserPresenceResponse>, Status> {
        let req = request.into_inner();
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        let tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?;

        debug!(
            tenant_id = %tenant_context.tenant_id,
            user_id = %req.user_id,
            "GetUserPresence request"
        );

        // TODO: 委托给Signaling Service查询在线状态
        Err(Status::unimplemented("GetUserPresence not yet implemented"))
    }

    /// 批量获取用户在线状态
    async fn batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> Result<Response<BatchGetUserPresenceResponse>, Status> {
        let _req = request.into_inner();
        debug!("BatchGetUserPresence request");

        // TODO: 委托给Signaling Service批量查询
        Err(Status::unimplemented("BatchGetUserPresence not yet implemented"))
    }

    /// 订阅用户状态变化
    async fn subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> Result<Response<SubscribeUserPresenceResponse>, Status> {
        let _req = request.into_inner();
        debug!("SubscribeUserPresence request");

        // TODO: 实现流式订阅
        Err(Status::unimplemented("SubscribeUserPresence not yet implemented"))
    }

    /// 列出用户设备
    async fn list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> Result<Response<ListUserDevicesResponse>, Status> {
        let _req = request.into_inner();
        debug!("ListUserDevices request");

        // TODO: 委托给Signaling Service查询设备列表
        Err(Status::unimplemented("ListUserDevices not yet implemented"))
    }

    /// 踢出设备
    async fn kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> Result<Response<KickDeviceResponse>, Status> {
        let _req = request.into_inner();
        debug!("KickDevice request");

        // TODO: 委托给Signaling Service踢出设备
        Err(Status::unimplemented("KickDevice not yet implemented"))
    }

    /// 获取设备信息
    async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> Result<Response<GetDeviceResponse>, Status> {
        let _req = request.into_inner();
        debug!("GetDevice request");

        // TODO: 委托给Signaling Service查询设备信息
        Err(Status::unimplemented("GetDevice not yet implemented"))
    }
}
*/

