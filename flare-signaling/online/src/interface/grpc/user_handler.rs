//! UserService gRPC Handler

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::domain::repository::PresenceWatcher;
use crate::domain::service::UserDomainService;
use flare_proto::signaling::user_service_server::UserService;
use flare_proto::signaling::*;
use prost_types::Timestamp;

#[derive(Clone)]
pub struct UserHandler {
    domain_service: Arc<UserDomainService>,
    presence_watcher: Arc<dyn PresenceWatcher>,
}

impl UserHandler {
    pub fn new(
        domain_service: Arc<UserDomainService>,
        presence_watcher: Arc<dyn PresenceWatcher>,
    ) -> Self {
        Self {
            domain_service,
            presence_watcher,
        }
    }

    pub async fn handle_get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> std::result::Result<Response<GetUserPresenceResponse>, Status> {
        match self.domain_service.get_user_presence(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_user_presence failed");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> std::result::Result<Response<BatchGetUserPresenceResponse>, Status> {
        match self.domain_service.batch_get_user_presence(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "batch_get_user_presence failed");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> std::result::Result<Response<ReceiverStream<std::result::Result<UserPresenceEvent, Status>>>, Status> {
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
                        let presence_event = UserPresenceEvent {
                            user_id: event.user_id.clone(),
                            is_online: event.status.online,
                            device_id: event.status.device_id.unwrap_or_default(),
                            timestamp: Some(Timestamp {
                                seconds: event.occurred_at.timestamp(),
                                nanos: event.occurred_at.timestamp_subsec_nanos() as i32,
                            }),
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

    pub async fn handle_list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> std::result::Result<Response<ListUserDevicesResponse>, Status> {
        match self.domain_service.list_user_devices(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "list_user_devices failed");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> std::result::Result<Response<KickDeviceResponse>, Status> {
        match self.domain_service.kick_device(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "kick_device failed");
                Err(Status::internal(err.to_string()))
            }
        }
    }

    pub async fn handle_get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> std::result::Result<Response<GetDeviceResponse>, Status> {
        match self.domain_service.get_device(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_device failed");
                Err(Status::internal(err.to_string()))
            }
        }
    }
}

#[tonic::async_trait]
impl UserService for UserHandler {
    async fn get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> std::result::Result<Response<GetUserPresenceResponse>, Status> {
        self.handle_get_user_presence(request).await
    }

    async fn batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> std::result::Result<Response<BatchGetUserPresenceResponse>, Status> {
        self.handle_batch_get_user_presence(request).await
    }

    type SubscribeUserPresenceStream = ReceiverStream<std::result::Result<UserPresenceEvent, Status>>;

    async fn subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> std::result::Result<Response<Self::SubscribeUserPresenceStream>, Status> {
        self.handle_subscribe_user_presence(request).await
    }

    async fn list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> std::result::Result<Response<ListUserDevicesResponse>, Status> {
        self.handle_list_user_devices(request).await
    }

    async fn kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> std::result::Result<Response<KickDeviceResponse>, Status> {
        self.handle_kick_device(request).await
    }

    async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> std::result::Result<Response<GetDeviceResponse>, Status> {
        self.handle_get_device(request).await
    }
}

