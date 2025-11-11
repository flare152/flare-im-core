use crate::handler::StorageWriterHandler;
use std::sync::Arc;
use tracing::info;

pub struct StorageWriter {
    handler: Arc<StorageWriterHandler>,
}

impl StorageWriter {
    pub async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let handler = Arc::new(StorageWriterHandler::new().await?);
        Ok(Self { handler })
    }

    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Storage Writer started, consuming from Kafka");
        self.handler.consume_messages().await?;
        Ok(())
    }
}
