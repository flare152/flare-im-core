pub mod noop;

use std::sync::Arc;
use async_trait::async_trait;

use crate::domain::repository::OnlinePushSender;
use crate::config::PushWorkerConfig;
use crate::domain::model::PushDispatchTask;
use flare_server_core::error::{Result, ErrorBuilder, ErrorCode};

pub type OnlinePushSenderRef = Arc<dyn OnlinePushSender>;

pub fn build_online_sender(config: &PushWorkerConfig) -> OnlinePushSenderRef {
    // 检查是否配置了access_gateway_service
    if config.access_gateway_service.is_some() {
        RealOnlinePushSender::shared()
    } else {
        noop::NoopOnlinePushSender::shared()
    }
}

pub use noop::NoopOnlinePushSender;

// 实际的在线推送发送器
pub struct RealOnlinePushSender;

impl RealOnlinePushSender {
    pub fn shared() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl OnlinePushSender for RealOnlinePushSender {
    async fn send(&self, task: &PushDispatchTask) -> Result<()> {
        // 在线推送逻辑应该是通过Gateway Router发送消息到Access Gateway
        // 但由于我们在这个模块中无法直接访问Gateway Router，
        // 我们会在领域服务中处理这部分逻辑
        
        // 这里只是一个占位实现，实际逻辑在PushDomainService::execute_online_push中
        tracing::info!(
            user_id = %task.user_id, 
            message_id = %task.message_id,
            "Real online push sender invoked"
        );
        
        // 模拟网络延迟
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        
        Ok(())
    }
}