//! 会话服务客户端

use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use flare_proto::session::session_service_client::SessionServiceClient;
use flare_proto::session::*;

use flare_server_core::discovery::ServiceClient;

/// gRPC会话服务客户端
pub struct GrpcSessionClient {
    /// 服务客户端（用于服务发现）
    service_client: Option<Arc<Mutex<ServiceClient>>>,
    /// 服务名称
    service_name: String,
    /// 直连地址（当没有服务发现时使用）
    direct_address: Option<String>,
}

impl GrpcSessionClient {
    /// 创建新的gRPC会话服务客户端
    pub fn new(service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: None,
        }
    }

    /// 使用服务客户端创建gRPC会话服务客户端
    pub fn with_service_client(service_client: ServiceClient, service_name: String) -> Self {
        Self {
            service_client: Some(Arc::new(Mutex::new(service_client))),
            service_name,
            direct_address: None,
        }
    }

    /// 使用直接地址创建gRPC会话服务客户端
    pub fn with_direct_address(direct_address: String, service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: Some(direct_address),
        }
    }

    /// 获取gRPC客户端
    async fn get_client(&self) -> Result<SessionServiceClient<Channel>, Status> {
        if let Some(service_client) = &self.service_client {
            let mut client = service_client.lock().await;
            let channel = client.get_channel().await.map_err(|e| {
                Status::unavailable(format!(
                    "Failed to get channel from service discovery: {}",
                    e
                ))
            })?;
            Ok(SessionServiceClient::new(channel))
        } else if let Some(ref address) = self.direct_address {
            let channel = Channel::from_shared(address.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| {
                    Status::unavailable(format!("Failed to connect to {}: {}", address, e))
                })?;
            Ok(SessionServiceClient::new(channel))
        } else {
            // 使用服务名称进行直连（假设服务名称可以直接解析）
            let channel = Channel::from_shared(self.service_name.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid service name: {}", e)))?
                .connect()
                .await
                .map_err(|e| {
                    Status::unavailable(format!(
                        "Failed to connect to {}: {}",
                        self.service_name, e
                    ))
                })?;
            Ok(SessionServiceClient::new(channel))
        }
    }

    /// 会话引导
    pub async fn session_bootstrap(
        &self,
        request: Request<SessionBootstrapRequest>,
    ) -> Result<Response<SessionBootstrapResponse>, Status> {
        let mut client = self.get_client().await?;
        client.session_bootstrap(request).await
    }

    /// 列出会话
    pub async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.list_sessions(request).await
    }

    /// 同步消息
    pub async fn sync_messages(
        &self,
        request: Request<SyncMessagesRequest>,
    ) -> Result<Response<SyncMessagesResponse>, Status> {
        let mut client = self.get_client().await?;
        client.sync_messages(request).await
    }

    /// 会话增量同步
    pub async fn sync_sessions(
        &self,
        request: Request<flare_proto::common::SyncSessionsRequest>,
    ) -> Result<Response<flare_proto::common::SyncSessionsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.sync_sessions(request).await
    }

    /// 会话全量恢复
    pub async fn get_all_sessions(
        &self,
        request: Request<flare_proto::common::SessionSyncAllRequest>,
    ) -> Result<Response<flare_proto::common::SessionSyncAllResponse>, Status> {
        let mut client = self.get_client().await?;
        client.get_all_sessions(request).await
    }

    /// 更新游标
    pub async fn update_cursor(
        &self,
        request: Request<UpdateCursorRequest>,
    ) -> Result<Response<UpdateCursorResponse>, Status> {
        let mut client = self.get_client().await?;
        client.update_cursor(request).await
    }

    /// 更新设备在线状态
    pub async fn update_presence(
        &self,
        request: Request<UpdatePresenceRequest>,
    ) -> Result<Response<UpdatePresenceResponse>, Status> {
        let mut client = self.get_client().await?;
        client.update_presence(request).await
    }

    /// 强制会话同步
    pub async fn force_session_sync(
        &self,
        request: Request<ForceSessionSyncRequest>,
    ) -> Result<Response<ForceSessionSyncResponse>, Status> {
        let mut client = self.get_client().await?;
        client.force_session_sync(request).await
    }

    /// 创建会话
    pub async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        let mut client = self.get_client().await?;
        client.create_session(request).await
    }

    /// 更新会话
    pub async fn update_session(
        &self,
        request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        let mut client = self.get_client().await?;
        client.update_session(request).await
    }

    /// 删除会话
    pub async fn delete_session(
        &self,
        request: Request<DeleteSessionRequest>,
    ) -> Result<Response<DeleteSessionResponse>, Status> {
        let mut client = self.get_client().await?;
        client.delete_session(request).await
    }

    /// 管理参与者
    pub async fn manage_participants(
        &self,
        request: Request<ManageParticipantsRequest>,
    ) -> Result<Response<ManageParticipantsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.manage_participants(request).await
    }

    /// 批量确认
    pub async fn batch_acknowledge(
        &self,
        request: Request<BatchAcknowledgeRequest>,
    ) -> Result<Response<BatchAcknowledgeResponse>, Status> {
        let mut client = self.get_client().await?;
        client.batch_acknowledge(request).await
    }

    /// 搜索会话
    pub async fn search_sessions(
        &self,
        request: Request<SearchSessionsRequest>,
    ) -> Result<Response<SearchSessionsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.search_sessions(request).await
    }

    /// 创建话题
    pub async fn create_thread(
        &self,
        request: Request<CreateThreadRequest>,
    ) -> Result<Response<CreateThreadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.create_thread(request).await
    }

    /// 获取话题列表
    pub async fn list_threads(
        &self,
        request: Request<ListThreadsRequest>,
    ) -> Result<Response<ListThreadsResponse>, Status> {
        let mut client = self.get_client().await?;
        client.list_threads(request).await
    }

    /// 获取话题详情
    pub async fn get_thread(
        &self,
        request: Request<GetThreadRequest>,
    ) -> Result<Response<GetThreadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.get_thread(request).await
    }

    /// 更新话题
    pub async fn update_thread(
        &self,
        request: Request<UpdateThreadRequest>,
    ) -> Result<Response<UpdateThreadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.update_thread(request).await
    }

    /// 删除话题
    pub async fn delete_thread(
        &self,
        request: Request<DeleteThreadRequest>,
    ) -> Result<Response<DeleteThreadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.delete_thread(request).await
    }
}
