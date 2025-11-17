use std::sync::Arc;

use flare_proto::message::message_service_server::MessageServiceServer;

use crate::interface::grpc::handler::MessageGrpcHandler;

#[derive(Clone)]
pub struct MessageGrpcServer {
    handler: Arc<MessageGrpcHandler>,
}

impl MessageGrpcServer {
    pub fn new(handler: Arc<MessageGrpcHandler>) -> Self {
        Self { handler }
    }

    pub fn into_service(self: &Arc<Self>) -> MessageServiceServer<MessageGrpcHandler> {
        MessageServiceServer::new((*self.handler).clone())
    }

    pub fn handler(&self) -> Arc<MessageGrpcHandler> {
        self.handler.clone()
    }
}

