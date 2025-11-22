//! 查询处理器（查询侧）- 部分查询直接调用基础设施层，包含业务逻辑的查询使用领域服务
//!
//! 在 CQRS 架构中，查询侧通常直接调用基础设施层（仓储实现），
//! 因为查询是只读操作，不涉及业务逻辑，不需要经过领域层。
//!
//! 注意：`get_metadata` 和 `create_presigned_url` 包含业务逻辑（缓存、默认值处理、URL构建），
//! 因此仍需要通过领域服务处理。

use std::sync::Arc;

use anyhow::Result;
use flare_proto::media::GetFileUrlRequest;

use crate::domain::model::{MediaFileMetadata, MediaReference, PresignedUrl};
use crate::domain::repository::ReferenceStoreRef;
use crate::domain::service::MediaService;

/// 媒体查询处理器（查询侧）
/// 
/// 简单查询直接使用基础设施层，包含业务逻辑的查询使用领域服务
pub struct MediaQueryHandler {
    // 包含业务逻辑的查询使用领域服务
    domain_service: Arc<MediaService>,
    // 简单查询直接使用基础设施层
    reference_store: Option<ReferenceStoreRef>,
}

impl MediaQueryHandler {
    pub fn new(
        domain_service: Arc<MediaService>,
        reference_store: Option<ReferenceStoreRef>,
    ) -> Self {
        Self {
            domain_service,
            reference_store,
        }
    }

    /// 获取文件信息（包含缓存逻辑，使用领域服务）
    pub async fn handle_get_file_info(&self, file_id: &str) -> Result<MediaFileMetadata> {
        self.domain_service.get_metadata(file_id).await
    }

    /// 获取文件URL（包含默认值处理和URL构建逻辑，使用领域服务）
    pub async fn handle_get_file_url(&self, request: GetFileUrlRequest) -> Result<PresignedUrl> {
        let expires_in = i64::from(request.expires_in);
        self.domain_service
            .create_presigned_url(&request.file_id, expires_in)
            .await
    }

    /// 列出文件引用（直接调用基础设施层）
    pub async fn handle_list_references(&self, file_id: &str) -> Result<Vec<MediaReference>> {
        if let Some(store) = &self.reference_store {
            store.list_references(file_id).await
        } else {
            Ok(vec![])
        }
    }

    pub fn to_proto_file_info(&self, metadata: &MediaFileMetadata) -> flare_proto::media::FileInfo {
        crate::application::utils::to_proto_file_info(metadata)
    }
}

