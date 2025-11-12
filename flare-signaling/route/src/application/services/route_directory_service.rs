use std::collections::HashMap;
use std::sync::Arc;

use flare_proto::signaling::{RouteMessageRequest, RouteMessageResponse};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use serde::{Deserialize, Serialize};

use crate::config::RouteConfig;
use crate::domain::repositories::RouteRepository;
use crate::util;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum RouteCommand {
    Lookup,
    Register { endpoint: String },
}

#[derive(Debug)]
enum RouteInstruction {
    Lookup,
    Register { endpoint: String },
    Forward { payload: Vec<u8> },
}

#[derive(Debug, Serialize)]
struct RouteLookupResponse {
    svid: String,
    endpoint: String,
}

pub struct RouteDirectoryService {
    repository: Arc<dyn RouteRepository>,
    cache: Arc<RwLock<HashMap<String, String>>>,
}

impl RouteDirectoryService {
    pub fn new(repository: Arc<dyn RouteRepository>, config: &RouteConfig) -> Self {
        let mut initial = HashMap::new();
        for (svid, endpoint) in &config.default_services {
            initial.insert(svid.clone(), endpoint.clone());
        }
        Self {
            repository,
            cache: Arc::new(RwLock::new(initial)),
        }
    }

    pub async fn register(&self, svid: String, endpoint: String) -> Result<()> {
        let endpoint = endpoint.trim().to_string();
        if endpoint.is_empty() {
            return Err(ErrorBuilder::new(
                ErrorCode::InvalidParameter,
                "endpoint must not be empty",
            )
            .build_error());
        }
        info!(%svid, %endpoint, "register business route");
        self.repository.upsert(&svid, &endpoint).await?;
        let mut map = self.cache.write().await;
        map.insert(svid, endpoint);
        Ok(())
    }

    pub async fn route(&self, request: RouteMessageRequest) -> Result<RouteMessageResponse> {
        let RouteMessageRequest {
            user_id,
            svid,
            payload,
            context: _,
            tenant: _,
        } = request;

        match Self::parse_instruction(payload) {
            RouteInstruction::Lookup => self.lookup_endpoint(&svid, &user_id).await,
            RouteInstruction::Register { endpoint } => {
                self.register(svid.clone(), endpoint).await?;
                info!(%svid, %user_id, "route registration completed");
                Ok(RouteMessageResponse {
                    success: true,
                    response: vec![],
                    error_message: String::new(),
                    status: util::rpc_status_ok(),
                })
            }
            RouteInstruction::Forward { .. } => {
                let message =
                    "business payload forwarding is not implemented in the current deployment"
                        .to_string();
                warn!(%svid, %user_id, "{}", message);
                Ok(RouteMessageResponse {
                    success: false,
                    response: vec![],
                    error_message: message.clone(),
                    status: util::rpc_status_error(ErrorCode::OperationNotSupported, &message),
                })
            }
        }
    }

    async fn resolve_cached(&self, svid: &str) -> Result<Option<String>> {
        if let Some(endpoint) = self.cache.read().await.get(svid).cloned() {
            return Ok(Some(endpoint));
        }

        if let Some(endpoint) = self.repository.resolve(svid).await? {
            let mut map = self.cache.write().await;
            map.insert(svid.to_string(), endpoint.clone());
            return Ok(Some(endpoint));
        }

        Ok(None)
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

        RouteInstruction::Forward { payload }
    }

    async fn lookup_endpoint(&self, svid: &str, user_id: &str) -> Result<RouteMessageResponse> {
        if let Some(endpoint) = self.resolve_cached(svid).await? {
            debug!(%svid, %user_id, %endpoint, "resolved business route endpoint");
            let body = serde_json::to_vec(&RouteLookupResponse {
                svid: svid.to_string(),
                endpoint,
            })
            .map_err(|err| {
                ErrorBuilder::new(
                    ErrorCode::SerializationError,
                    "failed to encode route response",
                )
                .details(err.to_string())
                .build_error()
            })?;

            return Ok(RouteMessageResponse {
                success: true,
                response: body,
                error_message: String::new(),
                status: util::rpc_status_ok(),
            });
        }

        let message = format!("business service not found for svid={}", svid);
        warn!(%svid, %user_id, "{}", message);
        Ok(RouteMessageResponse {
            success: false,
            response: vec![],
            error_message: message.clone(),
            status: util::rpc_status_error(ErrorCode::ServiceUnavailable, &message),
        })
    }
}
