use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::signaling::signaling_service_server::SignalingService;
use flare_proto::signaling::*;
use flare_server_core::error::ErrorCode;
use serde::{Deserialize, Serialize};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::{debug, error, info, warn};

use crate::application::handlers::{RouteCommandHandler, RouteQueryHandler};
use crate::application::commands::RegisterRouteCommand;
use crate::application::queries::ResolveRouteQuery;
use crate::infrastructure::forwarder::MessageForwarder;
use crate::domain::repository::RouteRepository;
use crate::util;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum RouteCommand {
    Lookup,
    Register { endpoint: String },
}

#[derive(Debug, Serialize)]
struct RouteLookupResponse {
    svid: String,
    endpoint: String,
}

#[derive(Clone)]
pub struct RouteHandler {
    command_handler: Arc<RouteCommandHandler>,
    query_handler: Arc<RouteQueryHandler>,
    forwarder: Arc<MessageForwarder>,
    repository: Arc<dyn RouteRepository + Send + Sync>,
}

impl RouteHandler {
    pub fn new(
        command_handler: Arc<RouteCommandHandler>,
        query_handler: Arc<RouteQueryHandler>,
        forwarder: Arc<MessageForwarder>,
        repository: Arc<dyn RouteRepository + Send + Sync>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
            forwarder,
            repository,
        }
    }

    pub async fn handle_route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        let req = request.into_inner();
        let RouteMessageRequest {
            user_id,
            svid,
            payload,
            context: _,
            tenant: _,
        } = req.clone();

        // 解析指令
        let instruction = Self::parse_instruction(payload);

        match instruction {
            RouteInstruction::Lookup => {
                let query = ResolveRouteQuery { svid: svid.clone() };
                match self.query_handler.handle_resolve_route(query).await {
                    Ok(Some(endpoint)) => {
                        debug!(%svid, %user_id, %endpoint, "resolved business route endpoint");
                        let body = serde_json::to_vec(&RouteLookupResponse {
                            svid: svid.clone(),
                            endpoint: endpoint.clone(),
                        })
                        .map_err(|err| {
                            Status::internal(format!("failed to encode route response: {}", err))
                        })?;

                        Ok(Response::new(RouteMessageResponse {
                            success: true,
                            response: body,
                            error_message: String::new(),
                            status: util::rpc_status_ok(),
                        }))
                    }
                    Ok(None) => {
                        let message = format!("business service not found for svid={}", svid);
                        warn!(%svid, %user_id, "{}", message);
                        Ok(Response::new(RouteMessageResponse {
                            success: false,
                            response: vec![],
                            error_message: message.clone(),
                            status: util::rpc_status_error(ErrorCode::ServiceUnavailable, &message),
                        }))
                    }
                    Err(err) => {
                        error!(error = ?err, "failed to resolve route");
                        Err(Status::internal(err.to_string()))
                    }
                }
            }
            RouteInstruction::Register { endpoint } => {
                let command = RegisterRouteCommand {
                    svid: svid.clone(),
                    endpoint: endpoint.clone(),
                };
                match self.command_handler.handle_register_route(command).await {
                    Ok(()) => {
                        info!(%svid, %user_id, "route registration completed");
                        Ok(Response::new(RouteMessageResponse {
                            success: true,
                            response: vec![],
                            error_message: String::new(),
                            status: util::rpc_status_ok(),
                        }))
                    }
                    Err(err) => {
                        error!(error = ?err, "failed to register route");
                        Err(Status::internal(err.to_string()))
                    }
                }
            }
            RouteInstruction::Forward { payload: forward_payload } => {
                info!(%svid, %user_id, payload_len = forward_payload.len(), "Forwarding message to business system");
                
                // 转发消息到业务系统
                match self.forwarder.forward_message(
                    &svid,
                    forward_payload,
                    req.context.clone(),
                    req.tenant.clone(),
                    Some(self.repository.clone()),
                ).await {
                    Ok(response_bytes) => {
                        info!(%svid, %user_id, "Message forwarded successfully");
                        Ok(Response::new(RouteMessageResponse {
                            success: true,
                            response: response_bytes,
                            error_message: String::new(),
                            status: util::rpc_status_ok(),
                        }))
                    }
                    Err(err) => {
                        error!(error = ?err, %svid, %user_id, "Failed to forward message");
                        Ok(Response::new(RouteMessageResponse {
                            success: false,
                            response: vec![],
                            error_message: format!("Failed to forward message: {}", err),
                            status: util::rpc_status_error(ErrorCode::InternalError, &format!("{}", err)),
                        }))
                    }
                }
            }
        }
    }

    fn parse_instruction(payload: Vec<u8>) -> RouteInstruction {
        if payload.is_empty() {
            return RouteInstruction::Lookup;
        }

        if payload.starts_with(b"{") {
            match serde_json::from_slice::<RouteCommand>(&payload) {
                Ok(RouteCommand::Lookup) => return RouteInstruction::Lookup,
                Ok(RouteCommand::Register { endpoint }) => {
                    return RouteInstruction::Register { endpoint };
                }
                Err(err) => {
                    debug!(error = %err, "failed to parse route command JSON, falling back to forward");
                }
            }
        }

        // 如果不是 JSON 命令，则作为消息转发处理
        RouteInstruction::Forward { payload }
    }
}

#[tonic::async_trait]
impl SignalingService for RouteHandler {
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
        self.handle_route_message(request).await
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

#[derive(Debug)]
enum RouteInstruction {
    Lookup,
    Register { endpoint: String },
    Forward { payload: Vec<u8> },
}
