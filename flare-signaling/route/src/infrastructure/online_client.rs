//! Online 服务 gRPC 客户端
//!
//! Router 通过此客户端调用 Online 服务的 OnlineService 接口查询设备信息

use anyhow::Result;
use flare_proto::signaling::online::{
    ListUserDevicesRequest, ListUserDevicesResponse,
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
}
