//! # SessionService Handler
//!
//! 提供会话管理、同步等能力，委托给Session Service。

use std::sync::Arc;

use anyhow::Result;
// TODO: 等待business.proto生成Rust代码后启用
// use flare_proto::business::session_service_server::SessionService;
// use flare_proto::business::{
//     BatchSyncMessagesRequest, BatchSyncMessagesResponse, CreateSessionRequest,
//     CreateSessionResponse, GetSessionRequest, GetSessionResponse, ListSessionsRequest,
//     ListSessionsResponse, SessionBootstrapRequest, SessionBootstrapResponse,
//     SyncMessagesRequest, SyncMessagesResponse,
// };
use tonic::{Request, Response, Status};
use tracing::debug;

use crate::interface::interceptor::{extract_claims, extract_tenant_context};

/// SessionService Handler实现
pub struct SessionServiceHandler {
    // Session Service客户端（TODO: 需要实现）
    // session_client: Arc<dyn SessionClient>,
}

impl SessionServiceHandler {
    /// 创建SessionService Handler
    pub fn new() -> Self {
        Self {
            // session_client,
        }
    }
}

// TODO: 等待business.proto生成Rust代码后启用
/*
#[tonic::async_trait]
impl SessionService for SessionServiceHandler {
    /// 会话初始化（多端同步）
    async fn session_bootstrap(
        &self,
        request: Request<SessionBootstrapRequest>,
    ) -> Result<Response<SessionBootstrapResponse>, Status> {
        let _req = request.into_inner();
        let _claims = extract_claims(&request)
            .ok_or_else(|| Status::unauthenticated("Missing authentication"))?;
        let _tenant_context = extract_tenant_context(&request)
            .ok_or_else(|| Status::invalid_argument("Missing tenant context"))?;

        debug!("SessionBootstrap request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("SessionBootstrap not yet implemented"))
    }

    /// 获取会话列表
    async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let _req = request.into_inner();
        debug!("ListSessions request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("ListSessions not yet implemented"))
    }

    /// 获取会话详情
    async fn get_session(
        &self,
        request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        let _req = request.into_inner();
        debug!("GetSession request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("GetSession not yet implemented"))
    }

    /// 创建会话
    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        let _req = request.into_inner();
        debug!("CreateSession request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("CreateSession not yet implemented"))
    }

    /// 同步消息（增量拉取）
    async fn sync_messages(
        &self,
        request: Request<SyncMessagesRequest>,
    ) -> Result<Response<SyncMessagesResponse>, Status> {
        let _req = request.into_inner();
        debug!("SyncMessages request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("SyncMessages not yet implemented"))
    }

    /// 批量同步消息
    async fn batch_sync_messages(
        &self,
        request: Request<BatchSyncMessagesRequest>,
    ) -> Result<Response<BatchSyncMessagesResponse>, Status> {
        let _req = request.into_inner();
        debug!("BatchSyncMessages request");

        // TODO: 委托给Session Service
        Err(Status::unimplemented("BatchSyncMessages not yet implemented"))
    }
}
*/

