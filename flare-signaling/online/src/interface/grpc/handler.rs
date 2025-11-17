use std::sync::Arc;

use flare_proto::signaling::*;
use flare_server_core::error::ErrorCode;
use prost_types::Timestamp;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::{OnlineStatusService, SubscriptionService};
use crate::domain::repositories::PresenceWatcher;
use crate::util;

pub struct OnlineHandler {
    service: Arc<OnlineStatusService>,
    subscription_service: Arc<SubscriptionService>,
    presence_watcher: Arc<dyn PresenceWatcher>,
}

impl OnlineHandler {
    pub fn new(
        service: Arc<OnlineStatusService>,
        subscription_service: Arc<SubscriptionService>,
        presence_watcher: Arc<dyn PresenceWatcher>,
    ) -> Self {
        Self {
            service,
            subscription_service,
            presence_watcher,
        }
    }

    pub async fn handle_login(
        &self,
        request: Request<LoginRequest>,
    ) -> std::result::Result<Response<LoginResponse>, Status> {
        match self.service.login(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "login failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> std::result::Result<Response<LogoutResponse>, Status> {
        match self.service.logout(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "logout failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> std::result::Result<Response<HeartbeatResponse>, Status> {
        let req = request.into_inner();
        match self.service.heartbeat(&req.session_id, &req.user_id).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "heartbeat failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> std::result::Result<Response<GetOnlineStatusResponse>, Status> {
        let req = request.into_inner();
        match self.service.get_online_status(&req.user_ids).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_online_status failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_route_message(
        &self,
        _request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        Ok(Response::new(RouteMessageResponse {
            success: false,
            response: vec![],
            error_message: "Route not supported in Online service".to_string(),
            status: util::rpc_status_error(ErrorCode::InvalidParameter, "route not supported"),
        }))
    }

    pub async fn handle_subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> std::result::Result<Response<SubscribeResponse>, Status> {
        match self.subscription_service.subscribe(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "subscribe failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_unsubscribe(
        &self,
        request: Request<UnsubscribeRequest>,
    ) -> std::result::Result<Response<UnsubscribeResponse>, Status> {
        match self.subscription_service.unsubscribe(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "unsubscribe failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_publish_signal(
        &self,
        request: Request<PublishSignalRequest>,
    ) -> std::result::Result<Response<PublishSignalResponse>, Status> {
        match self.subscription_service.publish_signal(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "publish_signal failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_watch_presence(
        &self,
        request: Request<WatchPresenceRequest>,
    ) -> std::result::Result<Response<ReceiverStream<std::result::Result<PresenceEvent, Status>>>, Status> {
        let req = request.into_inner();
        let user_ids = req.user_ids;

        if user_ids.is_empty() {
            return Err(Status::invalid_argument("user_ids is empty"));
        }

        // 创建事件接收器
        let mut receiver = match self.presence_watcher.watch_presence(&user_ids).await {
            Ok(rx) => rx,
            Err(err) => {
                error!(?err, "failed to watch presence");
                return Err(Status::internal(err.to_string()));
            }
        };

        // 创建发送器用于流式响应
        let (stream_tx, mut stream_rx) = mpsc::channel(100);
        
        // 启动后台任务转发事件
        tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Some(Ok(event)) => {
                        let presence_event = PresenceEvent {
                    user_id: event.user_id,
                    status: Some(OnlineStatus {
                        online: event.status.online,
                        server_id: event.status.server_id,
                        cluster_id: event.status.cluster_id.unwrap_or_default(),
                        last_seen: event.status.last_seen.as_ref().map(|dt| Timestamp {
                            seconds: dt.timestamp(),
                            nanos: dt.timestamp_subsec_nanos() as i32,
                        }),
                        tenant: None,
                        device_id: event.status.device_id.unwrap_or_default(),
                        device_platform: event.status.device_platform.unwrap_or_default(),
                        gateway_id: event.status.gateway_id.unwrap_or_default(), // 返回 gateway_id 用于跨地区路由
                    }),
                    occurred_at: Some(Timestamp {
                        seconds: event.occurred_at.timestamp(),
                        nanos: event.occurred_at.timestamp_subsec_nanos() as i32,
                    }),
                    conflict_action: event.conflict_action.unwrap_or(0),
                    reason: event.reason.unwrap_or_default(),
                };

                        if stream_tx.send(Ok(presence_event)).await.is_err() {
                            break;
                        }
                    }
                    Some(Err(_)) | None => break,
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(stream_rx)))
    }
}
