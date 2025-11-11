use std::sync::Arc;

use flare_proto::signaling::{GetOnlineStatusRequest, GetOnlineStatusResponse};
use flare_server_core::error::{Result, ok_status};

use crate::domain::SignalingGateway;

pub struct GetOnlineStatusQuery {
    pub request: GetOnlineStatusRequest,
}

pub struct SessionQueryService {
    signaling: Arc<dyn SignalingGateway>,
}

impl SessionQueryService {
    pub fn new(signaling: Arc<dyn SignalingGateway>) -> Self {
        Self { signaling }
    }

    pub async fn handle_get_online_status(
        &self,
        query: GetOnlineStatusQuery,
    ) -> Result<GetOnlineStatusResponse> {
        let mut response = self.signaling.get_online_status(query.request).await?;
        if response.status.is_none() {
            response.status = Some(ok_status());
        }
        Ok(response)
    }
}
