use std::sync::Arc;

use flare_proto::signaling::*;
use tonic::{Request, Response, Status};
use tracing::error;

use crate::application::RouteDirectoryService;

pub struct RouteHandler {
    service: Arc<RouteDirectoryService>,
}

impl RouteHandler {
    pub fn new(service: Arc<RouteDirectoryService>) -> Self {
        Self { service }
    }

    pub async fn handle_route_message(
        &self,
        request: Request<RouteMessageRequest>,
    ) -> std::result::Result<Response<RouteMessageResponse>, Status> {
        match self.service.route(request.into_inner()).await {
            Ok(response) => Ok(Response::new(response)),
            Err(err) => {
                error!(?err, "route message failed");
                Err(Status::from(err))
            }
        }
    }
}
