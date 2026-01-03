use std::sync::Arc;

use anyhow::Result;
use chrono::{DateTime, Utc};

use crate::domain::model::{
    MediaAssetStatus, MediaFileMetadata, MediaReference, UploadContext, UploadSession,
};

#[async_trait::async_trait]
pub trait MediaObjectRepository: Send + Sync {
    async fn put_object(&self, context: &UploadContext<'_>) -> Result<String>;
    async fn delete_object(&self, object_path: &str) -> Result<()>;
    async fn presign_object(&self, object_path: &str, expires_in: i64) -> Result<String>;
    fn base_url(&self) -> Option<String>;
    fn cdn_base_url(&self) -> Option<String>;
    fn use_presigned_urls(&self) -> bool;
    fn bucket_name(&self) -> Option<String> {
        None
    }
    fn storage_provider(&self) -> Option<String> {
        None
    }
}

#[async_trait::async_trait]
pub trait MediaMetadataStore: Send + Sync {
    async fn save_metadata(&self, metadata: &MediaFileMetadata) -> Result<()>;
    async fn load_metadata(&self, tenant_id: &str, file_id: &str) -> Result<Option<MediaFileMetadata>>;
    async fn load_by_hash(&self, sha256: &str) -> Result<Option<MediaFileMetadata>>;
    async fn delete_metadata(&self, file_id: &str) -> Result<()>;
    async fn list_orphaned_assets(&self, before: DateTime<Utc>) -> Result<Vec<MediaFileMetadata>>;
    async fn update_status(
        &self,
        file_id: &str,
        status: MediaAssetStatus,
        grace_expires_at: Option<DateTime<Utc>>,
    ) -> Result<()>;
}

#[async_trait::async_trait]
pub trait MediaMetadataCache: Send + Sync {
    async fn cache_metadata(&self, metadata: &MediaFileMetadata) -> Result<()>;
    async fn get_cached_metadata(&self, file_id: &str) -> Result<Option<MediaFileMetadata>>;
    async fn invalidate(&self, file_id: &str) -> Result<()>;
}

#[async_trait::async_trait]
pub trait MediaLocalStore: Send + Sync {
    async fn write(&self, context: &UploadContext<'_>) -> Result<String>;
    async fn delete(&self, file_id: &str) -> Result<()>;
    fn base_url(&self) -> Option<String>;
}

#[async_trait::async_trait]
pub trait MediaReferenceStore: Send + Sync {
    async fn create_reference(&self, reference: &MediaReference) -> Result<bool>;
    async fn delete_reference(&self, reference_id: &str) -> Result<bool>;
    async fn delete_any_reference(&self, tenant_id: &str, file_id: &str) -> Result<Option<String>>;
    async fn delete_all_references(&self, tenant_id: &str, file_id: &str) -> Result<u64>;
    async fn list_references(&self, tenant_id: &str, file_id: &str) -> Result<Vec<MediaReference>>;
    async fn count_references(&self, tenant_id: &str, file_id: &str) -> Result<u64>;
    async fn reference_exists(
        &self,
        tenant_id: &str,
        file_id: &str,
        namespace: &str,
        owner_id: &str,
        business_tag: Option<&str>,
    ) -> Result<bool>;
}

#[async_trait::async_trait]
pub trait UploadSessionStore: Send + Sync {
    async fn create_session(&self, session: &UploadSession) -> Result<()>;
    async fn get_session(&self, upload_id: &str) -> Result<Option<UploadSession>>;
    async fn upsert_session(&self, session: &UploadSession) -> Result<()>;
    async fn delete_session(&self, upload_id: &str) -> Result<()>;
}

pub type MetadataStoreRef = Arc<dyn MediaMetadataStore>;
pub type MetadataCacheRef = Arc<dyn MediaMetadataCache>;
pub type ObjectRepositoryRef = Arc<dyn MediaObjectRepository>;
pub type LocalStoreRef = Arc<dyn MediaLocalStore>;
pub type ReferenceStoreRef = Arc<dyn MediaReferenceStore>;
pub type UploadSessionStoreRef = Arc<dyn UploadSessionStore>;
