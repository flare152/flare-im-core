use async_trait::async_trait;
use flare_server_core::error::Result;

#[async_trait]
pub trait RouteRepository: Send + Sync {
    async fn upsert(&self, svid: &str, endpoint: &str) -> Result<()>;
    async fn resolve(&self, svid: &str) -> Result<Option<String>>;
}
