use std::collections::HashMap;

use crate::domain::model::{
    ConflictResolutionPolicy, DeviceState, SessionParticipant, SessionVisibility,
    SessionLifecycleState,
};

/// 批量确认命令
#[derive(Debug, Clone)]
pub struct BatchAcknowledgeCommand {
    pub user_id: String,
    pub cursors: Vec<(String, i64)>,
}

/// 创建会话命令
#[derive(Debug, Clone)]
pub struct CreateSessionCommand {
    pub session_type: String,
    pub business_type: String,
    pub participants: Vec<SessionParticipant>,
    pub attributes: HashMap<String, String>,
    pub visibility: SessionVisibility,
}

/// 删除会话命令
#[derive(Debug, Clone)]
pub struct DeleteSessionCommand {
    pub session_id: String,
    pub hard_delete: bool,
}

/// 强制会话同步命令
#[derive(Debug, Clone)]
pub struct ForceSessionSyncCommand {
    pub user_id: String,
    pub session_ids: Vec<String>,
    pub reason: Option<String>,
}

/// 管理参与者命令
#[derive(Debug, Clone)]
pub struct ManageParticipantsCommand {
    pub session_id: String,
    pub to_add: Vec<SessionParticipant>,
    pub to_remove: Vec<String>,
    pub role_updates: Vec<(String, Vec<String>)>,
}

/// 更新游标命令
#[derive(Debug, Clone)]
pub struct UpdateCursorCommand {
    pub user_id: String,
    pub session_id: String,
    pub message_ts: i64,
}

/// 更新设备状态命令
#[derive(Debug, Clone)]
pub struct UpdatePresenceCommand {
    pub user_id: String,
    pub device_id: String,
    pub device_platform: Option<String>,
    pub state: DeviceState,
    pub conflict_resolution: Option<ConflictResolutionPolicy>,
    pub notify_conflict: bool,
    pub conflict_reason: Option<String>,
}

/// 更新会话命令
#[derive(Debug, Clone)]
pub struct UpdateSessionCommand {
    pub session_id: String,
    pub display_name: Option<String>,
    pub attributes: Option<HashMap<String, String>>,
    pub visibility: Option<SessionVisibility>,
    pub lifecycle_state: Option<SessionLifecycleState>,
}

