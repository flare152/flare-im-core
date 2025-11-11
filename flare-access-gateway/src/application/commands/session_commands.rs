use std::sync::Arc;

use flare_proto::signaling::{
    HeartbeatRequest, HeartbeatResponse, LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result, ok_status, to_rpc_status};
use tracing::{info, warn};

use crate::domain::{Session, SessionStore, SignalingGateway};

pub struct LoginCommand {
    pub request: LoginRequest,
}

pub struct LogoutCommand {
    pub request: LogoutRequest,
}

pub struct HeartbeatCommand {
    pub request: HeartbeatRequest,
}

pub struct SessionCommandService {
    signaling: Arc<dyn SignalingGateway>,
    store: Arc<dyn SessionStore>,
    gateway_id: String,
}

impl SessionCommandService {
    pub fn new(
        signaling: Arc<dyn SignalingGateway>,
        store: Arc<dyn SessionStore>,
        gateway_id: String,
    ) -> Self {
        Self {
            signaling,
            store,
            gateway_id,
        }
    }

    pub async fn handle_login(&self, command: LoginCommand) -> Result<LoginResponse> {
        let mut response = self.signaling.login(command.request.clone()).await?;

        if response.success {
            let session = Session::new(
                response.session_id.clone(),
                command.request.user_id,
                command.request.device_id,
                Some(response.route_server.clone()),
                self.gateway_id.clone(),
            );
            self.store.insert(session).await?;
            info!(session_id = %response.session_id, "session registered");
            if response.status.is_none() {
                response.status = Some(ok_status());
            }
        } else {
            warn!(error = %response.error_message, "login failed via signaling");
            if response.status.is_none() {
                let error =
                    ErrorBuilder::new(ErrorCode::AuthenticationFailed, "signaling login rejected")
                        .details(response.error_message.clone())
                        .build_error();
                response.status = Some(to_rpc_status(&error));
            }
        }

        Ok(response)
    }

    pub async fn handle_logout(
        &self,
        command: LogoutCommand,
    ) -> Result<(LogoutResponse, Option<Session>)> {
        let mut response = self.signaling.logout(command.request.clone()).await?;

        let removed = if response.success {
            let removed = self.store.remove(&command.request.session_id).await?;
            if let Some(ref session) = removed {
                info!(session_id = %session.session_id, user_id = %session.user_id, "session removed");
            }
            removed
        } else {
            None
        };

        if response.status.is_none() {
            if response.success {
                response.status = Some(ok_status());
            } else {
                let error =
                    ErrorBuilder::new(ErrorCode::OperationFailed, "logout failed via signaling")
                        .build_error();
                response.status = Some(to_rpc_status(&error));
            }
        }

        Ok((response, removed))
    }

    pub async fn handle_heartbeat(&self, command: HeartbeatCommand) -> Result<HeartbeatResponse> {
        let mut response = self.signaling.heartbeat(command.request.clone()).await?;

        if response.success {
            let _ = self.store.touch(&command.request.session_id).await?;
            if response.status.is_none() {
                response.status = Some(ok_status());
            }
        } else if response.status.is_none() {
            let error = ErrorBuilder::new(
                ErrorCode::OperationFailed,
                "heartbeat rejected by signaling",
            )
            .build_error();
            response.status = Some(to_rpc_status(&error));
        }

        Ok(response)
    }

    pub fn store(&self) -> Arc<dyn SessionStore> {
        self.store.clone()
    }
}
