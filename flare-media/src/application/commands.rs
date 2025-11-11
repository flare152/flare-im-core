use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::media::{
    AbortMultipartUploadRequest, CompleteMultipartUploadRequest, DeleteFileRequest,
    InitiateMultipartUploadRequest, ProcessImageRequest, ProcessVideoRequest, UploadFileMetadata,
    UploadMultipartChunkRequest,
};
use tracing::info;
use uuid::Uuid;

use crate::application::service::{MediaApplication, to_proto_file_info};
use crate::domain::models::{MediaFileMetadata, MediaReferenceScope};

pub struct MediaCommandService {
    application: Arc<MediaApplication>,
}

pub struct ProcessedMediaResult {
    pub file_id: String,
    pub url: String,
    pub cdn_url: String,
}

impl MediaCommandService {
    pub fn new(application: Arc<MediaApplication>) -> Self {
        Self { application }
    }

    pub async fn upload_file(
        &self,
        metadata: UploadFileMetadata,
        payload: Vec<u8>,
    ) -> Result<MediaFileMetadata> {
        let file_id = Uuid::new_v4().to_string();
        self.application
            .upload_file(&metadata, &file_id, &payload)
            .await
    }

    pub async fn delete_file(&self, request: DeleteFileRequest) -> Result<()> {
        self.application.delete_file(&request.file_id).await
    }

    pub async fn initiate_multipart_upload(
        &self,
        request: InitiateMultipartUploadRequest,
    ) -> Result<crate::domain::models::MultipartUploadSession> {
        let metadata = request.metadata.context("multipart metadata missing")?;

        let desired_chunk_size = if request.desired_chunk_size > 0 {
            request.desired_chunk_size
        } else {
            8 * 1024 * 1024
        };

        self.application
            .start_multipart_upload(metadata, desired_chunk_size)
            .await
    }

    pub async fn upload_multipart_chunk(
        &self,
        request: UploadMultipartChunkRequest,
    ) -> Result<crate::domain::models::MultipartUploadSession> {
        if request.upload_id.is_empty() {
            anyhow::bail!("upload_id is required");
        }
        if request.payload.is_empty() {
            anyhow::bail!("chunk payload is empty");
        }

        self.application
            .upload_multipart_chunk(&request.upload_id, request.chunk_index, request.payload)
            .await
    }

    pub async fn complete_multipart_upload(
        &self,
        request: CompleteMultipartUploadRequest,
    ) -> Result<MediaFileMetadata> {
        self.application
            .complete_multipart_upload(&request.upload_id)
            .await
    }

    pub async fn abort_multipart_upload(&self, request: AbortMultipartUploadRequest) -> Result<()> {
        self.application
            .abort_multipart_upload(&request.upload_id)
            .await
    }

    pub async fn attach_reference(
        &self,
        file_id: &str,
        scope: MediaReferenceScope,
        metadata: HashMap<String, String>,
    ) -> Result<MediaFileMetadata> {
        self.application
            .attach_reference(file_id, scope, metadata)
            .await
    }

    pub async fn release_reference(
        &self,
        file_id: &str,
        reference_id: Option<&str>,
    ) -> Result<MediaFileMetadata> {
        self.application
            .release_reference(file_id, reference_id)
            .await
    }

    pub async fn cleanup_orphaned_assets(&self) -> Result<Vec<String>> {
        self.application.cleanup_orphaned_assets().await
    }

    pub async fn process_image(
        &self,
        request: ProcessImageRequest,
    ) -> Result<ProcessedMediaResult> {
        info!(
            file_id = %request.file_id,
            operations = request.operations.len(),
            "Process image request received"
        );
        // TODO: Integrate real image pipeline
        Ok(ProcessedMediaResult {
            file_id: format!("processed_{}", request.file_id),
            url: String::new(),
            cdn_url: String::new(),
        })
    }

    pub async fn process_video(
        &self,
        request: ProcessVideoRequest,
    ) -> Result<ProcessedMediaResult> {
        info!(
            file_id = %request.file_id,
            operations = request.operations.len(),
            "Process video request received"
        );
        // TODO: Integrate real video pipeline
        Ok(ProcessedMediaResult {
            file_id: format!("processed_{}", request.file_id),
            url: String::new(),
            cdn_url: String::new(),
        })
    }

    pub fn to_proto_file_info(&self, metadata: &MediaFileMetadata) -> flare_proto::media::FileInfo {
        to_proto_file_info(metadata)
    }
}
