//! # Hook配置加载器
//!
//! 提供从不同源加载Hook配置的能力

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::domain::model::HookConfig;
use crate::infrastructure::persistence::postgres_config::PostgresHookConfigRepository;

/// Hook配置加载器接口
#[async_trait]
pub trait ConfigLoader: Send + Sync {
    /// 加载Hook配置
    async fn load(&self) -> Result<HookConfig>;
}

/// Hook配置合并器
///
/// 按优先级合并多个配置源的Hook配置
pub struct ConfigMerger;

impl ConfigMerger {
    /// 合并多个配置
    ///
    /// 优先级：动态API（数据库） > 配置中心 > 配置文件
    /// 同名Hook高优先级覆盖低优先级
    pub fn merge(configs: Vec<HookConfig>) -> HookConfig {
        let mut merged = HookConfig::default();
        
        // 按顺序合并，后面的配置会覆盖前面的同名配置
        for config in configs {
            Self::merge_hook_list(&mut merged.pre_send, config.pre_send);
            Self::merge_hook_list(&mut merged.post_send, config.post_send);
            Self::merge_hook_list(&mut merged.delivery, config.delivery);
            Self::merge_hook_list(&mut merged.recall, config.recall);
            Self::merge_hook_list(&mut merged.session_create, config.session_create);
            Self::merge_hook_list(&mut merged.session_update, config.session_update);
            Self::merge_hook_list(&mut merged.session_delete, config.session_delete);
            Self::merge_hook_list(&mut merged.user_login, config.user_login);
            Self::merge_hook_list(&mut merged.user_logout, config.user_logout);
            Self::merge_hook_list(&mut merged.user_online, config.user_online);
            Self::merge_hook_list(&mut merged.user_offline, config.user_offline);
            Self::merge_hook_list(&mut merged.push_pre_send, config.push_pre_send);
            Self::merge_hook_list(&mut merged.push_post_send, config.push_post_send);
            Self::merge_hook_list(&mut merged.push_delivery, config.push_delivery);
        }
        
        merged
    }

    fn merge_hook_list(target: &mut Vec<crate::domain::model::HookConfigItem>, source: Vec<crate::domain::model::HookConfigItem>) {
        for hook in source {
            if let Some(existing) = target.iter_mut().find(|h| h.name == hook.name) {
                // 高优先级覆盖
                *existing = hook;
            } else {
                target.push(hook);
            }
        }
    }
}

/// Hook配置验证器
pub struct ConfigValidator;

impl ConfigValidator {
    /// 验证Hook配置
    pub fn validate(config: &HookConfig) -> Result<()> {
        // 验证所有Hook类型
        let all_hooks = config.pre_send.iter()
            .chain(config.post_send.iter())
            .chain(config.delivery.iter())
            .chain(config.recall.iter())
            .chain(config.session_create.iter())
            .chain(config.session_update.iter())
            .chain(config.session_delete.iter())
            .chain(config.user_login.iter())
            .chain(config.user_logout.iter())
            .chain(config.user_online.iter())
            .chain(config.user_offline.iter())
            .chain(config.push_pre_send.iter())
            .chain(config.push_post_send.iter())
            .chain(config.push_delivery.iter());
        
        for hook in all_hooks {
            Self::validate_hook(hook)?;
        }
        
        Ok(())
    }
    
    fn validate_hook(hook: &crate::domain::model::HookConfigItem) -> Result<()> {
        if hook.name.is_empty() {
            anyhow::bail!("Hook name cannot be empty");
        }
        
        if hook.priority < 0 || hook.priority > 1000 {
            anyhow::bail!("Hook priority must be between 0 and 1000");
        }
        
        if hook.timeout_ms == 0 || hook.timeout_ms > 30000 {
            anyhow::bail!("Hook timeout must be between 1ms and 30000ms");
        }
        
        Ok(())
    }
}

/// 配置文件加载器
pub struct FileConfigLoader {
    path: PathBuf,
}

impl FileConfigLoader {
    pub fn new<P: Into<PathBuf>>(path: P) -> Self {
        Self {
            path: path.into(),
        }
    }
}

#[async_trait]
impl ConfigLoader for FileConfigLoader {
    async fn load(&self) -> Result<HookConfig> {
        if !self.path.exists() {
            debug!(path = %self.path.display(), "Hook config file not found, using default config");
            return Ok(HookConfig::default());
        }
        
        let content = tokio::fs::read_to_string(&self.path)
            .await
            .with_context(|| format!("Failed to read hook config file: {}", self.path.display()))?;
        
        let config: HookConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse hook config file: {}", self.path.display()))?;
        
        debug!(
            path = %self.path.display(),
            hooks_count = config.pre_send.len() + config.post_send.len() + config.delivery.len() + config.recall.len(),
            "Loaded hook config from file"
        );
        
        Ok(config)
    }
}

/// 数据库配置加载器（动态API配置）
pub struct DatabaseConfigLoader {
    repository: Arc<PostgresHookConfigRepository>,
    tenant_id: Option<String>,
}

impl DatabaseConfigLoader {
    pub fn new(repository: Arc<PostgresHookConfigRepository>, tenant_id: Option<String>) -> Self {
        Self {
            repository,
            tenant_id,
        }
    }
}

#[async_trait]
impl ConfigLoader for DatabaseConfigLoader {
    async fn load(&self) -> Result<HookConfig> {
        let config = self.repository
            .load_all(self.tenant_id.as_deref())
            .await
            .context("Failed to load hook config from database")?;
        
        info!(
            tenant_id = ?self.tenant_id,
            hooks_count = config.pre_send.len() + config.post_send.len() + config.delivery.len() + config.recall.len(),
            "Loaded hook config from database"
        );
        
        Ok(config)
    }
}

/// 配置中心加载器（etcd/Consul）
pub struct ConfigCenterLoader {
    endpoint: String,
    tenant_id: Option<String>,
    config_key: String,
}

impl ConfigCenterLoader {
    pub fn new(endpoint: String, tenant_id: Option<String>) -> Self {
        // 构建配置键：/flare/hooks/{tenant_id}/config 或 /flare/hooks/config
        let config_key = if let Some(ref tenant) = tenant_id {
            format!("/flare/hooks/{}/config", tenant)
        } else {
            "/flare/hooks/config".to_string()
        };
        
        Self {
            endpoint,
            tenant_id,
            config_key,
        }
    }

    /// 解析endpoint，支持etcd://host:port格式
    fn parse_endpoint(&self) -> Result<(String, u16)> {
        if self.endpoint.starts_with("etcd://") {
            let addr = self.endpoint.strip_prefix("etcd://").unwrap();
            let parts: Vec<&str> = addr.split(':').collect();
            if parts.len() != 2 {
                anyhow::bail!("Invalid etcd endpoint format: {}", self.endpoint);
            }
            let host = parts[0].to_string();
            let port = parts[1].parse::<u16>()
                .context("Invalid etcd port")?;
            Ok((host, port))
        } else if self.endpoint.starts_with("consul://") {
            // TODO: 支持Consul
            anyhow::bail!("Consul not implemented yet");
        } else {
            anyhow::bail!("Unsupported config center endpoint format: {}", self.endpoint);
        }
    }
}

#[async_trait]
impl ConfigLoader for ConfigCenterLoader {
    async fn load(&self) -> Result<HookConfig> {
        let (host, port) = self.parse_endpoint()?;
        
        // 使用etcd-client连接etcd并读取配置
        use etcd_client::Client;
        
        let endpoints = vec![format!("http://{}:{}", host, port)];
        let mut client = Client::connect(endpoints, None)
            .await
            .context("Failed to connect to etcd")?;
        
        let resp = client.get(self.config_key.clone(), None)
            .await
            .context("Failed to get config from etcd")?;
        
        if let Some(kv) = resp.kvs().first() {
            // etcd-client返回的是bytes，需要转换为字符串
            let value = String::from_utf8(kv.value().to_vec())
                .context("Failed to parse etcd value as UTF-8 string")?;
            
            // 解析配置（支持TOML或JSON格式）
            let config: HookConfig = if value.trim_start().starts_with('{') {
                // JSON格式
                serde_json::from_str(&value)
                    .context("Failed to parse etcd config as JSON")?
            } else {
                // TOML格式
                toml::from_str(&value)
                    .context("Failed to parse etcd config as TOML")?
            };
            
            info!(
                endpoint = %self.endpoint,
                config_key = %self.config_key,
                hooks_count = config.pre_send.len() + config.post_send.len() + config.delivery.len() + config.recall.len(),
                "Loaded hook config from etcd"
            );
            
            Ok(config)
        } else {
            warn!(
                endpoint = %self.endpoint,
                config_key = %self.config_key,
                "Config not found in etcd, using default config"
            );
            Ok(HookConfig::default())
        }
    }
}
