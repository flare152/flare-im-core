use std::sync::Arc;

use anyhow::Result;
use flare_proto::media::GetFileUrlRequest;

use crate::application::service::{MediaApplication, to_proto_file_info};
use crate::domain::models::{MediaFileMetadata, MediaReference, PresignedUrl};

pub struct MediaQueryService {
    application: Arc<MediaApplication>,
}

impl MediaQueryService {
    pub fn new(application: Arc<MediaApplication>) -> Self {
        Self { application }
    }

    pub async fn get_file_info(&self, file_id: &str) -> Result<MediaFileMetadata> {
        self.application.get_file_info(file_id).await
    }

    pub async fn get_file_url(&self, request: GetFileUrlRequest) -> Result<PresignedUrl> {
        let expires_in = i64::from(request.expires_in);
        self.application
            .get_presigned_url(&request.file_id, expires_in)
            .await
    }

    pub async fn list_references(&self, file_id: &str) -> Result<Vec<MediaReference>> {
        self.application.list_references(file_id).await
    }

    pub fn to_proto_file_info(&self, metadata: &MediaFileMetadata) -> flare_proto::media::FileInfo {
        to_proto_file_info(metadata)
    }
}
