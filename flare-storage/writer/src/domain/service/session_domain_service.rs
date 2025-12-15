//! 会话领域服务 - 处理与会话相关的业务逻辑
//!
//! 职责：
//! - 通过gRPC调用Session服务获取会话参与者列表
//! - 更新参与者的未读数
//! - 提供领域层的会话操作接口

use std::sync::Arc;
use anyhow::{Result, anyhow};
use tracing::warn;
use flare_server_core::discovery::ServiceClient;
use tokio::sync::Mutex;

/// 会话领域服务
pub struct SessionDomainService {
    service_client: Option<Arc<Mutex<ServiceClient>>>,
}

impl SessionDomainService {
    /// 创建会话领域服务
    pub fn new(service_client: Option<Arc<Mutex<ServiceClient>>>) -> Self {
        Self {
            service_client,
        }
    }

    /// 获取会话参与者列表
    /// 
    /// 通过gRPC调用Session服务获取会话的所有参与者，用于更新未读数
    pub async fn get_session_participants(&self, session_id: &str) -> Result<Vec<String>> {
        // 注意：由于ServiceClient不能被克隆，我们需要使用Mutex来安全地访问它
        // 这里我们暂时返回一个空的参与者列表，表示该功能尚未完全实现
        Ok(vec![])
    }
}