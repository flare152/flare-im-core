//! UserService gRPC Handler

use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::UserService as UserServiceApp;
use crate::domain::repositories::PresenceWatcher;
use flare_proto::signaling::*;
use prost_types::Timestamp;

pub struct UserHandler {
    service: Arc<UserServiceApp>,
    presence_watcher: Arc<dyn PresenceWatcher>,
}

impl UserHandler {
    pub fn new(
        service: Arc<UserServiceApp>,
        presence_watcher: Arc<dyn PresenceWatcher>,
    ) -> Self {
        Self {
            service,
            presence_watcher,
        }
    }

    pub async fn handle_get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> std::result::Result<Response<GetUserPresenceResponse>, Status> {
        match self.service.get_user_presence(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_user_presence failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> std::result::Result<Response<BatchGetUserPresenceResponse>, Status> {
        match self.service.batch_get_user_presence(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "batch_get_user_presence failed");
                Err(Status::from(err))
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
        match self.service.list_user_devices(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "list_user_devices failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> std::result::Result<Response<KickDeviceResponse>, Status> {
        match self.service.kick_device(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "kick_device failed");
                Err(Status::from(err))
            }
        }
    }

    pub async fn handle_get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> std::result::Result<Response<GetDeviceResponse>, Status> {
        match self.service.get_device(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "get_device failed");
                Err(Status::from(err))
            }
        }
    }
}

