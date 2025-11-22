use anyhow::{Result, anyhow};
use async_trait::async_trait;
use flare_proto::media::media_service_client::MediaServiceClient;
use flare_proto::{GetFileInfoRequest, RequestContext, TenantContext};
use tonic::transport::Channel;
use tracing::warn;

use crate::domain::model::MediaAttachmentMetadata;
use crate::domain::repository::MediaAttachmentVerifier;

pub struct MediaAttachmentClient {
    endpoint: String,
    client: tokio::sync::Mutex<Option<MediaServiceClient<Channel>>>,
}

impl MediaAttachmentClient {
    pub fn new(endpoint: String) -> Self {
        Self {
            endpoint,
            client: tokio::sync::Mutex::new(None),
        }
    }

    async fn ensure_client(&self) -> Result<MediaServiceClient<Channel>> {
        let mut guard = self.client.lock().await;
        if let Some(client) = guard.as_ref() {
            return Ok(client.clone());
        }

        let client = MediaServiceClient::connect(self.endpoint.clone())
            .await
            .map_err(|err| anyhow!("Failed to connect media service: {err}"))?;

        *guard = Some(client.clone());
        Ok(client)
    }
}

#[async_trait]
impl MediaAttachmentVerifier for MediaAttachmentClient {
    async fn fetch_metadata(&self, file_ids: &[String]) -> Result<Vec<MediaAttachmentMetadata>> {
        let mut client = self.ensure_client().await?;
        let mut result = Vec::with_capacity(file_ids.len());

        for file_id in file_ids {
            let request = GetFileInfoRequest {
                file_id: file_id.clone(),
                context: Some(RequestContext::default()),
                tenant: Some(TenantContext::default()),
            };

            match client.get_file_info(tonic::Request::new(request)).await {
                Ok(response) => {
                    if let Some(info) = response.into_inner().info {
                        result.push(MediaAttachmentMetadata {
                            file_id: info.file_id,
                            file_name: info.file_name,
                            mime_type: info.mime_type,
                            size: info.size,
                            url: info.url,
                            cdn_url: info.cdn_url,
                        });
                    } else {
                        warn!(file_id = %file_id, "Media info missing in response");
                    }
                }
                Err(err) => {
                    warn!(error = ?err, file_id = %file_id, "Media service returned error");
                }
            }
        }

        Ok(result)
    }
}
