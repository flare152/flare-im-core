//! Online 服务 gRPC 客户端
//!
//! Gateway 通过此客户端调用 Online 服务的 OnlineService 接口查询设备信息

use anyhow::Result;
use flare_proto::signaling::online::{
    GetDeviceRequest, GetDeviceResponse, ListUserDevicesRequest, ListUserDevicesResponse,
    online_service_client::OnlineServiceClient as ProtoOnlineServiceClient,
};
use flare_proto::{RequestContext, TenantContext};
use tonic::transport::Channel;

pub struct OnlineServiceClient {
    client: ProtoOnlineServiceClient<Channel>,
}

impl OnlineServiceClient {
    pub async fn new(endpoint: String) -> Result<Self> {
        let channel = Channel::from_shared(endpoint)?.connect().await?;

        let client = ProtoOnlineServiceClient::new(channel);

        Ok(Self { client })
    }

    /// 查询用户的所有在线设备
    pub async fn list_user_devices(&self, user_id: &str) -> Result<ListUserDevicesResponse> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(ListUserDevicesRequest {
            user_id: user_id.to_string(),
            context: Some(RequestContext::default()),
            tenant: Some(TenantContext::default()),
        });

        let response = client.list_user_devices(request).await?;
        Ok(response.into_inner())
    }

    /// 获取设备信息
    pub async fn get_device(&self, user_id: &str, device_id: &str) -> Result<GetDeviceResponse> {
        let mut client = self.client.clone();

        let request = tonic::Request::new(GetDeviceRequest {
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            context: Some(RequestContext::default()),
            tenant: Some(TenantContext::default()),
        });

        let response = client.get_device(request).await?;
        Ok(response.into_inner())
    }

    /// 获取底层的 OnlineServiceClient
    pub fn get_online_service_client(&self) -> ProtoOnlineServiceClient<Channel> {
        self.client.clone()
    }
}
