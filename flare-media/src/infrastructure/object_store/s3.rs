use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Credentials, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::error::ProvideErrorMetadata;

use crate::domain::models::UploadContext;
use crate::domain::repositories::MediaObjectRepository;
use flare_im_core::config::ObjectStoreConfig;

#[derive(Clone)]
pub struct S3ObjectStore {
    client: S3Client,
    bucket: String,
    base_url: Option<String>,
    cdn_base_url: Option<String>,
    upload_prefix: Option<String>,
    force_path_style: bool,
}

impl S3ObjectStore {
    pub async fn from_config(cfg: &ObjectStoreConfig) -> Result<Self> {
        tracing::debug!("Initializing S3ObjectStore with config: {:?}", cfg);
        
        let bucket = cfg
            .bucket
            .clone()
            .ok_or_else(|| anyhow!("object storage bucket is required"))?;

        let region = cfg
            .region
            .clone()
            .unwrap_or_else(|| "us-east-1".to_string());
        let region = Region::new(region);

        // Build base AWS config
        let region_provider = RegionProviderChain::first_try(region.clone());
        let mut loader = aws_config::from_env().region(region_provider);

        // If endpoint is provided, we are likely using an S3-compatible store (e.g. MinIO)
        let endpoint = cfg.endpoint.clone();
        let force_path_style = endpoint.is_some();

        // Credentials (access_key/secret_key), if provided, use static
        let aws_cfg = if let (Some(access_key), Some(secret_key)) =
            (cfg.access_key.clone(), cfg.secret_key.clone())
        {
            tracing::debug!("Using static credentials for S3 client");
            let credentials = Credentials::new(
                access_key,
                secret_key,
                None,
                None,
                "static-credentials",
            );
            loader = loader.credentials_provider(credentials);
            loader.load().await
        } else {
            tracing::debug!("Using default credentials provider for S3 client");
            loader.load().await
        };

        // Build S3 client config
        let mut s3_builder = S3ConfigBuilder::from(&aws_cfg).region(region);
        if let Some(ep) = endpoint.clone() {
            tracing::debug!("Setting S3 endpoint to: {}", ep);
            s3_builder = s3_builder.endpoint_url(ep);
        }
        if force_path_style {
            tracing::debug!("Forcing path style for S3 client");
            s3_builder = s3_builder.force_path_style(true);
        }
        
        tracing::debug!("Building S3 config");
        let s3_config = s3_builder.build();
        tracing::debug!("Creating S3 client");
        let client = S3Client::from_conf(s3_config);

        tracing::debug!("S3ObjectStore initialized successfully");
        Ok(Self {
            client,
            bucket,
            base_url: cfg.endpoint.clone(), // fallback: for constructing direct URLs when no CDN
            cdn_base_url: cfg.cdn_base_url.clone(),
            upload_prefix: cfg.upload_prefix.clone(),
            force_path_style,
        })
    }

    fn key_of(&self, file_id: &str) -> String {
        if let Some(prefix) = &self.upload_prefix {
            if prefix.is_empty() {
                file_id.to_string()
            } else {
                format!("{}/{}", prefix.trim_matches('/'), file_id)
            }
        } else {
            file_id.to_string()
        }
    }

    fn object_url(&self, key: &str) -> Option<String> {
        // Prefer CDN if available
        if let Some(cdn) = &self.cdn_base_url {
            if !cdn.is_empty() {
                return Some(format!("{}/{}", cdn.trim_end_matches('/'), key));
            }
        }
        // Construct a generic URL when endpoint is known and path-style is used
        if let Some(base) = &self.base_url {
            if !base.is_empty() {
                if self.force_path_style {
                    return Some(format!(
                        "{}/{}/{}",
                        base.trim_end_matches('/'),
                        self.bucket,
                        key
                    ));
                } else {
                    // virtual host style (may not work with custom endpoints without DNS)
                    return Some(format!(
                        "https://{}.s3.amazonaws.com/{}",
                        self.bucket, key
                    ));
                }
            }
        }
        None
    }
}

#[async_trait::async_trait]
impl MediaObjectRepository for S3ObjectStore {
    async fn put_object(&self, context: &UploadContext<'_>) -> Result<String> {
        let key = self.key_of(context.file_id);
        tracing::debug!(
            file_id = context.file_id,
            key = &key,
            bucket = &self.bucket,
            file_size = context.payload.len(),
            "开始上传对象到S3存储"
        );
        
        let bs = ByteStream::from(context.payload.to_vec());
        tracing::debug!(file_id = context.file_id, key = &key, bucket = &self.bucket, "准备调用S3 put_object API");
        
        let result = self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .content_type(context.mime_type)
            .body(bs)
            .send()
            .await;
            
        tracing::debug!(file_id = context.file_id, key = &key, bucket = &self.bucket, "S3 put_object API调用完成");

        match result {
            Ok(_) => {
                tracing::debug!(
                    file_id = context.file_id,
                    key = &key,
                    bucket = &self.bucket,
                    "对象已成功上传到S3存储"
                );
                Ok(key)
            }
            Err(e) => {
                tracing::error!(
                    file_id = context.file_id,
                    key = &key,
                    bucket = &self.bucket,
                    error = %e,
                    error_meta = ?e.meta(),
                    "上传对象到S3存储失败"
                );
                Err(anyhow::Error::from(e).context(format!("failed to upload object to s3, key={}, bucket={}", key, self.bucket)))
            }
        }
    }

    async fn delete_object(&self, file_id: &str) -> Result<()> {
        let key = self.key_of(file_id);
        tracing::debug!(file_id = file_id, key = &key, bucket = &self.bucket, "开始从S3存储删除对象");
        
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .with_context(|| format!("failed to delete object from s3, key={}", key))?;
            
        tracing::debug!(file_id = file_id, key = &key, bucket = &self.bucket, "对象已成功从S3存储删除");
        Ok(())
    }

    async fn presign_object(&self, file_id: &str, expires_in: i64) -> Result<String> {
        let key = self.key_of(file_id);
        tracing::debug!(file_id = file_id, key = &key, bucket = &self.bucket, expires_in = expires_in, "开始生成S3对象的预签名URL");
        
        // If CDN is configured, return constructed URL directly
        if let Some(url) = self.object_url(&key) {
            tracing::debug!(file_id = file_id, key = &key, cdn_url = &url, "使用CDN URL");
            return Ok(url);
        }
        
        // Otherwise, generate a presigned GET URL
        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .presigned(
                aws_sdk_s3::presigning::PresigningConfig::expires_in(Duration::from_secs(
                    (expires_in.max(1) as u64).min(7 * 24 * 3600),
                ))
                .map_err(|e| anyhow!("invalid presign config: {}", e))?,
            )
            .await
            .with_context(|| format!("failed to presign s3 url, key={}", key))?;
            
        let url = presigned.uri().to_string();
        tracing::debug!(file_id = file_id, key = &key, presigned_url = &url, "已生成预签名URL");
        Ok(url)
    }

    fn base_url(&self) -> Option<String> {
        self.base_url.clone()
    }

    fn cdn_base_url(&self) -> Option<String> {
        self.cdn_base_url.clone()
    }
}

pub type S3ObjectStoreRef = Arc<S3ObjectStore>;
