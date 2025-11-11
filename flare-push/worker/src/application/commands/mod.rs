use std::sync::Arc;

use flare_server_core::error::Result;
use tracing::{info, warn};

use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::{OfflinePushSender, OnlinePushSender};

pub struct PushExecutionCommandService {
    online_sender: Arc<dyn OnlinePushSender>,
    offline_sender: Arc<dyn OfflinePushSender>,
}

impl PushExecutionCommandService {
    pub fn new(
        online_sender: Arc<dyn OnlinePushSender>,
        offline_sender: Arc<dyn OfflinePushSender>,
    ) -> Self {
        Self {
            online_sender,
            offline_sender,
        }
    }

    pub async fn execute(&self, task: PushDispatchTask) -> Result<()> {
        if task.require_online && !task.online {
            info!(user_id = %task.user_id, "skip offline task due to require_online=true");
            return Ok(());
        }

        if task.online {
            self.online_sender.send(&task).await
        } else if task.persist_if_offline {
            self.offline_sender.send(&task).await
        } else {
            warn!(user_id = %task.user_id, "offline task dropped because persist_if_offline=false");
            Ok(())
        }
    }
}
