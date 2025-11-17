use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result as AnyhowResult};
use flare_im_core::config::FlareAppConfig;
use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use flare_server_core::Config;
use flare_server_core::error::{ErrorCode, Result};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::info;

use crate::application::RouteDirectoryService;
use crate::config::RouteConfig;
use crate::infrastructure::persistence::memory::InMemoryRouteRepository;
use crate::interface::grpc::handler::RouteHandler;
use crate::util;

#[derive(Clone)]
pub struct SignalingRouteServer {
    handler: Arc<RouteHandler>,
}

impl SignalingRouteServer {
    pub async fn new(app_config: &FlareAppConfig, _config: Config) -> AnyhowResult<Self> {
        let route_config = Arc::new(
            RouteConfig::from_app_config(app_config)
                .context("Failed to load route service configuration")?,
        );
        let repository = Arc::new(InMemoryRouteRepository::new(
            route_config.default_services.clone(),
        ));
        let service = RouteDirectoryService::new(repository, &route_config);
        for (svid, endpoint) in &route_config.default_services {
            service.register(svid.clone(), endpoint.clone()).await?;
        }
        let service = Arc::new(service);
        let handler = Arc::new(RouteHandler::new(service));
        Ok(Self { handler })
    }
}

#[tonic::async_trait]
impl SignalingService for SignalingRouteServer {
    async fn login(
        &self,
        _request: Request<LoginRequest>,
    ) -> std::result::Result<Response<LoginResponse>, Status> {
        Ok(Response::new(LoginResponse {
            success: false,
            session_id: String::new(),
            route_server: String::new(),
            error_message: "login not supported".to_string(),
            status: util::rpc_status_error(ErrorCode::OperationNotSupported, "login not supported"),
            applied_conflict_strategy: 0,
        }))
    }

    async fn logout(
        &self,
        _request: Request<LogoutRequest>,
    ) -> std::result::Result<Response<LogoutResponse>, Status> {
        Ok(Response::new(LogoutResponse {
            success: false,
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "logout not supported",
            ),
        }))
    }

    async fn heartbeat(
        &self,
        _request: Request<HeartbeatRequest>,
    ) -> std::result::Result<Response<HeartbeatResponse>, Status> {
        Ok(Response::new(HeartbeatResponse {
            success: false,
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "heartbeat not supported",
            ),
        }))
    }

    async fn get_online_status(
        &self,
        _request: Request<GetOnlineStatusRequest>,
    ) -> std::result::Result<Response<GetOnlineStatusResponse>, Status> {
        Ok(Response::new(GetOnlineStatusResponse {
            statuses: HashMap::new(),
            status: util::rpc_status_error(
                ErrorCode::OperationNotSupported,
                "online status not supported",
            ),
        }))
    }

    async fn route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        info!(user_id = %request.get_ref().user_id, svid = %request.get_ref().svid, "Route message request");
        self.handler.handle_route_message(request).await
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> std::result::Result<Response<SubscribeResponse>, Status> {
        Err(Status::unimplemented("subscribe not supported by route service"))
    }

    async fn unsubscribe(
        &self,
        _request: Request<UnsubscribeRequest>,
    ) -> std::result::Result<Response<UnsubscribeResponse>, Status> {
        Err(Status::unimplemented("unsubscribe not supported by route service"))
    }

    async fn publish_signal(
        &self,
        _request: Request<PublishSignalRequest>,
    ) -> std::result::Result<Response<PublishSignalResponse>, Status> {
        Err(Status::unimplemented("publish_signal not supported by route service"))
    }

    type WatchPresenceStream = ReceiverStream<std::result::Result<PresenceEvent, Status>>;

    async fn watch_presence(
        &self,
        _request: Request<WatchPresenceRequest>,
    ) -> std::result::Result<Response<Self::WatchPresenceStream>, Status> {
        Err(Status::unimplemented("watch_presence not supported by route service"))
    }
}
