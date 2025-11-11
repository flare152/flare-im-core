use std::sync::Arc;

use flare_proto::media::media_service_server::MediaServiceServer;

use crate::interface::grpc::handler::MediaGrpcHandler;

#[derive(Clone)]
pub struct MediaGrpcServer {
    handler: Arc<MediaGrpcHandler>,
}

impl MediaGrpcServer {
    pub fn new(handler: Arc<MediaGrpcHandler>) -> Self {
        Self { handler }
    }

    pub fn into_service(self: &Arc<Self>) -> MediaServiceServer<MediaGrpcHandler> {
        MediaServiceServer::new((*self.handler).clone())
    }

    pub fn handler(&self) -> Arc<MediaGrpcHandler> {
        self.handler.clone()
    }
}
