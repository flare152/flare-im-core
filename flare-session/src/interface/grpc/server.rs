use std::net::SocketAddr;

use anyhow::Result;
use flare_proto::session::session_service_server::SessionServiceServer;
use tonic::transport::Server;

use crate::interface::grpc::handler::SessionGrpcHandler;

pub struct GrpcServer {
    handler: SessionGrpcHandler,
    address: SocketAddr,
}

impl GrpcServer {
    pub fn new(handler: SessionGrpcHandler, address: SocketAddr) -> Self {
        Self { handler, address }
    }

    pub async fn run(&self) -> Result<()> {
        Server::builder()
            .add_service(SessionServiceServer::new(self.handler.clone()))
            .serve(self.address)
            .await?;
        Ok(())
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }
}
