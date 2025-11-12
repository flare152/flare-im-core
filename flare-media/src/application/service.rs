use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use flare_proto::media::UploadFileMetadata;
use prost_types::Timestamp;

use crate::domain::models::{
    FILE_CATEGORY_METADATA_KEY, MediaFileMetadata, MediaReference, MediaReferenceScope,
    MultipartChunkPayload, MultipartUploadInit, MultipartUploadSession, PresignedUrl,
    UploadContext, infer_file_category,
};
use crate::domain::service::MediaService;

pub struct MediaApplication {
    service: Arc<MediaService>,
}

impl MediaApplication {
    pub fn new(service: Arc<MediaService>) -> Self {
        Self { service }
    }

    pub async fn upload_file(
        &self,
        metadata: &UploadFileMetadata,
        file_id: &str,
        payload: &[u8],
    ) -> Result<MediaFileMetadata> {
        let mut extra_metadata = metadata.metadata.clone();

        if !metadata.namespace.is_empty() {
            extra_metadata
                .entry("namespace".to_string())
                .or_insert_with(|| metadata.namespace.clone());
        }

        if !metadata.business_tag.is_empty() {
            extra_metadata
                .entry("business_tag".to_string())
                .or_insert_with(|| metadata.business_tag.clone());
        }

        let trace_id = if metadata.trace_id.is_empty() {
            None
        } else {
            Some(metadata.trace_id.as_str())
        };

        let namespace = if metadata.namespace.is_empty() {
            None
        } else {
            Some(metadata.namespace.as_str())
        };

        let business_tag = if metadata.business_tag.is_empty() {
            None
        } else {
            Some(metadata.business_tag.as_str())
        };

        let file_category = infer_file_category(
            Some(metadata.file_type().as_str_name()),
            metadata.mime_type.as_str(),
        );
        extra_metadata
            .entry(FILE_CATEGORY_METADATA_KEY.to_string())
            .or_insert_with(|| file_category.clone());

        let context = UploadContext {
            file_id,
            file_name: metadata.file_name.as_str(),
            mime_type: metadata.mime_type.as_str(),
            payload,
            file_size: metadata.file_size,
            file_category,
            user_id: metadata.user_id.as_str(),
            trace_id,
            namespace,
            business_tag,
            metadata: extra_metadata,
        };

        self.service
            .store_media_file(context)
            .await
            .context("store media file")
    }

    pub async fn delete_file(&self, file_id: &str) -> Result<()> {
        self.service.delete_media_file(file_id).await
    }

    pub async fn get_file_info(&self, file_id: &str) -> Result<MediaFileMetadata> {
        self.service.get_metadata(file_id).await
    }

    pub async fn get_presigned_url(&self, file_id: &str, expires_in: i64) -> Result<PresignedUrl> {
        self.service.create_presigned_url(file_id, expires_in).await
    }

    pub async fn attach_reference(
        &self,
        file_id: &str,
        scope: MediaReferenceScope,
        metadata: HashMap<String, String>,
    ) -> Result<MediaFileMetadata> {
        self.service.add_reference(file_id, scope, metadata).await
    }

    pub async fn release_reference(
        &self,
        file_id: &str,
        reference_id: Option<&str>,
    ) -> Result<MediaFileMetadata> {
        self.service.remove_reference(file_id, reference_id).await
    }

    pub async fn list_references(&self, file_id: &str) -> Result<Vec<MediaReference>> {
        self.service.list_references(file_id).await
    }

    pub async fn cleanup_orphaned_assets(&self) -> Result<Vec<String>> {
        self.service.cleanup_orphaned_assets().await
    }

    pub async fn start_multipart_upload(
        &self,
        metadata: UploadFileMetadata,
        chunk_size: i64,
    ) -> Result<MultipartUploadSession> {
        let file_category = infer_file_category(
            Some(metadata.file_type().as_str_name()),
            metadata.mime_type.as_str(),
        );

        let mut metadata_map = metadata.metadata.clone();
        metadata_map
            .entry(FILE_CATEGORY_METADATA_KEY.to_string())
            .or_insert_with(|| file_category.clone());

        let init = MultipartUploadInit {
            file_name: metadata.file_name.clone(),
            mime_type: metadata.mime_type.clone(),
            file_size: if metadata.file_size > 0 {
                Some(metadata.file_size)
            } else {
                None
            },
            file_type: metadata.file_type().as_str_name().to_string(),
            chunk_size,
            user_id: metadata.user_id.clone(),
            namespace: if metadata.namespace.is_empty() {
                None
            } else {
                Some(metadata.namespace.clone())
            },
            business_tag: if metadata.business_tag.is_empty() {
                None
            } else {
                Some(metadata.business_tag.clone())
            },
            trace_id: if metadata.trace_id.is_empty() {
                None
            } else {
                Some(metadata.trace_id.clone())
            },
            metadata: metadata_map,
        };

        self.service
            .initiate_multipart_upload(init)
            .await
            .context("initiate multipart upload")
    }

    pub async fn upload_multipart_chunk(
        &self,
        upload_id: &str,
        chunk_index: u32,
        bytes: Vec<u8>,
    ) -> Result<MultipartUploadSession> {
        let payload = MultipartChunkPayload {
            upload_id: upload_id.to_string(),
            chunk_index,
            bytes,
        };

        self.service
            .upload_multipart_chunk(payload)
            .await
            .context("upload multipart chunk")
    }

    pub async fn complete_multipart_upload(&self, upload_id: &str) -> Result<MediaFileMetadata> {
        self.service
            .complete_multipart_upload(upload_id)
            .await
            .context("complete multipart upload")
    }

    pub async fn abort_multipart_upload(&self, upload_id: &str) -> Result<()> {
        self.service
            .abort_multipart_upload(upload_id)
            .await
            .context("abort multipart upload")
    }
}

pub fn to_proto_file_info(metadata: &MediaFileMetadata) -> flare_proto::media::FileInfo {
    flare_proto::media::FileInfo {
        file_id: metadata.file_id.clone(),
        file_name: metadata.file_name.clone(),
        mime_type: metadata.mime_type.clone(),
        size: metadata.file_size,
        url: metadata.url.clone(),
        cdn_url: metadata.cdn_url.clone(),
        metadata: metadata.metadata.clone(),
        created_at: Some(to_proto_timestamp(metadata.uploaded_at)),
        tenant: None,
        reference_count: metadata.reference_count,
        status: metadata.status.as_str().to_string(),
        sha256: metadata.sha256.clone().unwrap_or_default(),
        md5: metadata.md5.clone().unwrap_or_default(),
        grace_expires_at: metadata.grace_expires_at.map(to_proto_timestamp),
    }
}

fn to_proto_timestamp(value: chrono::DateTime<chrono::Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos() as i32,
    }
}

pub fn to_proto_reference(reference: &MediaReference) -> flare_proto::media::MediaReferenceInfo {
    flare_proto::media::MediaReferenceInfo {
        reference_id: reference.reference_id.clone(),
        file_id: reference.file_id.clone(),
        namespace: reference.namespace.clone(),
        owner_id: reference.owner_id.clone(),
        business_tag: reference.business_tag.clone().unwrap_or_default(),
        metadata: reference.metadata.clone(),
        created_at: Some(to_proto_timestamp(reference.created_at)),
        expires_at: reference.expires_at.map(to_proto_timestamp),
    }
}
