//! 领域服务

use std::sync::Arc;

use crate::domain::repositories::{OnlineStatusRepository, PushTaskPublisher};

/// 推送领域服务
pub struct PushService {
    online_repo: Arc<dyn OnlineStatusRepository>,
    task_publisher: Arc<dyn PushTaskPublisher>,
}

impl PushService {
    pub fn new(
        online_repo: Arc<dyn OnlineStatusRepository>,
        task_publisher: Arc<dyn PushTaskPublisher>,
    ) -> Self {
        Self {
            online_repo,
            task_publisher,
        }
    }

    /// 获取在线状态仓库
    pub fn online_repo(&self) -> Arc<dyn OnlineStatusRepository> {
        Arc::clone(&self.online_repo)
    }

    /// 获取任务发布器
    pub fn task_publisher(&self) -> Arc<dyn PushTaskPublisher> {
        Arc::clone(&self.task_publisher)
    }
}

