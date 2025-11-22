//! 命令处理器（编排层）- 负责处理命令，调用领域服务

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::media::{
    AbortMultipartUploadRequest, CompleteMultipartUploadRequest, DeleteFileRequest,
    InitiateMultipartUploadRequest, ProcessImageRequest, ProcessVideoRequest, UploadFileMetadata,
    UploadMultipartChunkRequest,
};
use tracing::info;

use crate::domain::model::{
    MediaFileMetadata, MediaReferenceScope, MultipartChunkPayload,
    MultipartUploadSession, UploadContext,
};
use crate::domain::service::MediaService;

/// 媒体命令处理器（编排层）
pub struct MediaCommandHandler {
    domain_service: Arc<MediaService>,
}

/// 处理后的媒体结果
pub struct ProcessedMediaResult {
    pub file_id: String,
    pub url: String,
    pub cdn_url: String,
}

impl MediaCommandHandler {
    pub fn new(domain_service: Arc<MediaService>) -> Self {
        Self { domain_service }
    }

    pub async fn handle_upload_file(
        &self,
        metadata: UploadFileMetadata,
        payload: Vec<u8>,
    ) -> Result<MediaFileMetadata> {
        // 使用领域服务准备上传上下文数据（业务逻辑下沉到领域层）
        let (file_id, file_category, extra_metadata, trace_id, namespace, business_tag) = 
            self.domain_service.prepare_upload_context_data(&metadata);
        
        // 构建 UploadContext（在命令处理器作用域中，确保生命周期正确）
        let context = UploadContext {
            file_id: &file_id,
            file_name: metadata.file_name.as_str(),
            mime_type: metadata.mime_type.as_str(),
            payload: &payload,
            file_size: metadata.file_size,
            file_category,
            user_id: metadata.user_id.as_str(),
            trace_id,
            namespace,
            business_tag,
            metadata: extra_metadata,
        };

        self.domain_service
            .store_media_file(context)
            .await
            .context("store media file")
    }

    pub async fn handle_delete_file(&self, request: DeleteFileRequest) -> Result<()> {
        self.domain_service.delete_media_file(&request.file_id).await
    }

    pub async fn handle_initiate_multipart_upload(
        &self,
        request: InitiateMultipartUploadRequest,
    ) -> Result<MultipartUploadSession> {
        // 使用领域服务准备分片上传初始化（业务逻辑下沉到领域层）
        let init = self.domain_service
            .prepare_multipart_upload_init(&request)
            .context("prepare multipart upload init")?;

        self.domain_service
            .initiate_multipart_upload(init)
            .await
            .context("initiate multipart upload")
    }

    pub async fn handle_upload_multipart_chunk(
        &self,
        request: UploadMultipartChunkRequest,
    ) -> Result<MultipartUploadSession> {
        if request.upload_id.is_empty() {
            anyhow::bail!("upload_id is required");
        }
        if request.payload.is_empty() {
            anyhow::bail!("chunk payload is empty");
        }

        let payload = MultipartChunkPayload {
            upload_id: request.upload_id.clone(),
            chunk_index: request.chunk_index,
            bytes: request.payload,
        };

        self.domain_service
            .upload_multipart_chunk(payload)
            .await
            .context("upload multipart chunk")
    }

    pub async fn handle_complete_multipart_upload(
        &self,
        request: CompleteMultipartUploadRequest,
    ) -> Result<MediaFileMetadata> {
        self.domain_service
            .complete_multipart_upload(&request.upload_id)
            .await
            .context("complete multipart upload")
    }

    pub async fn handle_abort_multipart_upload(
        &self,
        request: AbortMultipartUploadRequest,
    ) -> Result<()> {
        self.domain_service
            .abort_multipart_upload(&request.upload_id)
            .await
            .context("abort multipart upload")
    }

    pub async fn handle_attach_reference(
        &self,
        file_id: &str,
        scope: MediaReferenceScope,
        metadata: HashMap<String, String>,
    ) -> Result<MediaFileMetadata> {
        self.domain_service
            .add_reference(file_id, scope, metadata)
            .await
    }

    pub async fn handle_release_reference(
        &self,
        file_id: &str,
        reference_id: Option<&str>,
    ) -> Result<MediaFileMetadata> {
        self.domain_service
            .remove_reference(file_id, reference_id)
            .await
    }

    pub async fn handle_cleanup_orphaned_assets(&self) -> Result<Vec<String>> {
        self.domain_service.cleanup_orphaned_assets().await
    }

    pub async fn handle_process_image(
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

    pub async fn handle_process_video(
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
        crate::application::utils::to_proto_file_info(metadata)
    }
}

