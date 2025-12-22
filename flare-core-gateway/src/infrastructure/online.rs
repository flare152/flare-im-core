//! 在线状态服务客户端

use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tonic::{Request, Response, Status, Streaming};

use flare_proto::signaling::online::online_service_client::OnlineServiceClient;
use flare_proto::signaling::online::*;

use flare_server_core::discovery::ServiceClient;

/// gRPC在线状态服务客户端
pub struct GrpcOnlineClient {
    /// 服务客户端（用于服务发现）
    service_client: Option<Arc<Mutex<ServiceClient>>>,
    /// 服务名称
    service_name: String,
    /// 直连地址（当没有服务发现时使用）
    direct_address: Option<String>,
}

impl GrpcOnlineClient {
    /// 创建新的gRPC在线状态服务客户端
    pub fn new(service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: None,
        }
    }

    /// 使用服务客户端创建gRPC在线状态服务客户端
    pub fn with_service_client(service_client: ServiceClient, service_name: String) -> Self {
        Self {
            service_client: Some(Arc::new(Mutex::new(service_client))),
            service_name,
            direct_address: None,
        }
    }

    /// 使用直接地址创建gRPC在线状态服务客户端
    pub fn with_direct_address(direct_address: String, service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: Some(direct_address),
        }
    }

    /// 获取OnlineService gRPC客户端（统一的服务，包含 SignalingService 和 UserService 的所有功能）
    async fn get_online_client(&self) -> Result<OnlineServiceClient<Channel>, Status> {
        if let Some(service_client) = &self.service_client {
            let mut client = service_client.lock().await;
            let channel = client.get_channel().await.map_err(|e| {
                Status::unavailable(format!(
                    "Failed to get channel from service discovery: {}",
                    e
                ))
            })?;
            Ok(OnlineServiceClient::new(channel))
        } else if let Some(ref address) = self.direct_address {
            let channel = Channel::from_shared(address.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| {
                    Status::unavailable(format!("Failed to connect to {}: {}", address, e))
                })?;
            Ok(OnlineServiceClient::new(channel))
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
            Ok(OnlineServiceClient::new(channel))
        }
    }

    /// 用户登录
    pub async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.login(request).await
    }

    /// 用户登出
    pub async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.logout(request).await
    }

    /// 心跳
    pub async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.heartbeat(request).await
    }

    /// 获取在线状态
    pub async fn get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> Result<Response<GetOnlineStatusResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.get_online_status(request).await
    }

    /// 监听在线状态变化
    pub async fn watch_presence(
        &self,
        request: Request<WatchPresenceRequest>,
    ) -> Result<Response<Streaming<PresenceEvent>>, Status> {
        let mut client = self.get_online_client().await?;
        client.watch_presence(request).await
    }

    /// 查询用户在线状态
    pub async fn get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> Result<Response<GetUserPresenceResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.get_user_presence(request).await
    }

    /// 批量查询在线状态
    pub async fn batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> Result<Response<BatchGetUserPresenceResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.batch_get_user_presence(request).await
    }

    /// 订阅用户状态变化
    pub async fn subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> Result<Response<Streaming<UserPresenceEvent>>, Status> {
        let mut client = self.get_online_client().await?;
        client.subscribe_user_presence(request).await
    }

    /// 列出用户设备
    pub async fn list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> Result<Response<ListUserDevicesResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.list_user_devices(request).await
    }

    /// 踢出设备
    pub async fn kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> Result<Response<KickDeviceResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.kick_device(request).await
    }

    /// 查询设备信息
    pub async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> Result<Response<GetDeviceResponse>, Status> {
        let mut client = self.get_online_client().await?;
        client.get_device(request).await
    }
}
