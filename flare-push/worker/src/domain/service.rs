//! 领域服务

use std::sync::Arc;

use crate::domain::repositories::{OfflinePushSender, OnlinePushSender};

/// 推送领域服务
pub struct PushService {
    online_sender: Arc<dyn OnlinePushSender>,
    offline_sender: Arc<dyn OfflinePushSender>,
}

impl PushService {
    pub fn new(
        online_sender: Arc<dyn OnlinePushSender>,
        offline_sender: Arc<dyn OfflinePushSender>,
    ) -> Self {
        Self {
            online_sender,
            offline_sender,
        }
    }

    /// 获取在线推送发送器
    pub fn online_sender(&self) -> Arc<dyn OnlinePushSender> {
        Arc::clone(&self.online_sender)
    }

    /// 获取离线推送发送器
    pub fn offline_sender(&self) -> Arc<dyn OfflinePushSender> {
        Arc::clone(&self.offline_sender)
    }
}

