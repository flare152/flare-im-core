use chrono::{DateTime, TimeZone, Utc};
use std::collections::HashMap;

use flare_proto::session::{
    ConflictResolution as ProtoConflictResolution, DeviceState as ProtoDeviceState,
    SessionLifecycleState as ProtoSessionLifecycleState, SessionVisibility as ProtoSessionVisibility,
};
use flare_proto::common::Message;

#[derive(Clone, Debug)]
pub struct SessionSummary {
    pub session_id: String,
    pub session_type: Option<String>,
    pub business_type: Option<String>,
    pub last_message_id: Option<String>,
    pub last_message_time: Option<DateTime<Utc>>,
    pub last_sender_id: Option<String>,
    pub last_message_type: Option<i32>,
    pub last_content_type: Option<String>,
    pub unread_count: i32,
    pub metadata: HashMap<String, String>,
    pub server_cursor_ts: Option<i64>,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DevicePresence {
    pub device_id: String,
    pub device_platform: Option<String>,
    pub state: DeviceState,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum DeviceState {
    Unspecified,
    Online,
    Offline,
    Conflict,
}

impl DeviceState {
    pub fn from_str(state: &str) -> Self {
        match state {
            "online" => DeviceState::Online,
            "offline" => DeviceState::Offline,
            "conflict" => DeviceState::Conflict,
            _ => DeviceState::Unspecified,
        }
    }

    pub fn as_proto(&self) -> i32 {
        match self {
            DeviceState::Unspecified => ProtoDeviceState::Unspecified as i32,
            DeviceState::Online => ProtoDeviceState::Online as i32,
            DeviceState::Offline => ProtoDeviceState::Offline as i32,
            DeviceState::Conflict => ProtoDeviceState::Conflict as i32,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionBootstrapResult {
    pub summaries: Vec<SessionSummary>,
    pub recent_messages: Vec<Message>,
    pub cursor_map: HashMap<String, i64>,
    pub policy: SessionPolicy,
}

#[derive(Clone, Debug)]
pub struct MessageSyncResult {
    pub messages: Vec<Message>,
    pub next_cursor: Option<String>,
    pub server_cursor_ts: Option<i64>,
    /// 基于 seq 的游标（可选，用于优化性能）
    pub server_cursor_seq: Option<i64>,
}

pub fn millis_to_datetime(ms: i64) -> Option<DateTime<Utc>> {
    Some(Utc.timestamp_millis_opt(ms).single()?)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictResolutionPolicy {
    Unspecified,
    Exclusive,
    PlatformExclusive,
    Coexist,
    ForceLogout,
}

impl ConflictResolutionPolicy {
    pub fn as_proto(&self) -> i32 {
        match self {
            ConflictResolutionPolicy::Unspecified => {
                ProtoConflictResolution::Unspecified as i32
            }
            ConflictResolutionPolicy::Exclusive => {
                ProtoConflictResolution::Exclusive as i32
            }
            ConflictResolutionPolicy::PlatformExclusive => {
                ProtoConflictResolution::PlatformExclusive as i32
            }
            ConflictResolutionPolicy::Coexist => {
                ProtoConflictResolution::Coexist as i32
            }
            ConflictResolutionPolicy::ForceLogout => {
                ProtoConflictResolution::ForceLogout as i32
            }
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ConflictResolutionPolicy::Unspecified => "unspecified",
            ConflictResolutionPolicy::Exclusive => "exclusive",
            ConflictResolutionPolicy::PlatformExclusive => "platform_exclusive",
            ConflictResolutionPolicy::Coexist => "coexist",
            ConflictResolutionPolicy::ForceLogout => "force_logout",
        }
    }

    pub fn from_proto(value: i32) -> Self {
        match ProtoConflictResolution::try_from(value).ok() {
            Some(ProtoConflictResolution::Exclusive) => Self::Exclusive,
            Some(ProtoConflictResolution::PlatformExclusive) => {
                Self::PlatformExclusive
            }
            Some(ProtoConflictResolution::Coexist) => Self::Coexist,
            Some(ProtoConflictResolution::ForceLogout) => Self::ForceLogout,
            _ => Self::Unspecified,
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "exclusive" => Some(Self::Exclusive),
            "platform-exclusive" | "platform_exclusive" => Some(Self::PlatformExclusive),
            "coexist" => Some(Self::Coexist),
            "force-logout" | "force_logout" => Some(Self::ForceLogout),
            "unspecified" => Some(Self::Unspecified),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionPolicy {
    pub conflict_resolution: ConflictResolutionPolicy,
    pub max_devices: i32,
    pub allow_anonymous: bool,
    pub allow_history_sync: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub session_id: String,
    pub session_type: String,
    pub business_type: String,
    pub display_name: Option<String>,
    pub attributes: HashMap<String, String>,
    pub participants: Vec<SessionParticipant>,
    pub visibility: SessionVisibility,
    pub lifecycle_state: SessionLifecycleState,
    pub policy: Option<SessionPolicy>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct SessionParticipant {
    pub user_id: String,
    pub roles: Vec<String>,
    pub muted: bool,
    pub pinned: bool,
    pub attributes: HashMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionVisibility {
    Unspecified,
    Private,
    Tenant,
    Public,
}

impl SessionVisibility {
    pub fn from_proto(value: i32) -> Self {
        match ProtoSessionVisibility::try_from(value).ok() {
            Some(ProtoSessionVisibility::Private) => Self::Private,
            Some(ProtoSessionVisibility::Tenant) => Self::Tenant,
            Some(ProtoSessionVisibility::Public) => Self::Public,
            _ => Self::Unspecified,
        }
    }

    pub fn as_proto(&self) -> i32 {
        match self {
            SessionVisibility::Unspecified => ProtoSessionVisibility::Unspecified as i32,
            SessionVisibility::Private => ProtoSessionVisibility::Private as i32,
            SessionVisibility::Tenant => ProtoSessionVisibility::Tenant as i32,
            SessionVisibility::Public => ProtoSessionVisibility::Public as i32,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SessionVisibility::Unspecified => "unspecified",
            SessionVisibility::Private => "private",
            SessionVisibility::Tenant => "tenant",
            SessionVisibility::Public => "public",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionLifecycleState {
    Unspecified,
    Active,
    Suspended,
    Archived,
    Deleted,
}

impl SessionLifecycleState {
    pub fn from_proto(value: i32) -> Self {
        match ProtoSessionLifecycleState::try_from(value).ok() {
            Some(ProtoSessionLifecycleState::SessionLifecycleActive) => Self::Active,
            Some(ProtoSessionLifecycleState::SessionLifecycleSuspended) => Self::Suspended,
            Some(ProtoSessionLifecycleState::SessionLifecycleArchived) => Self::Archived,
            Some(ProtoSessionLifecycleState::SessionLifecycleDeleted) => Self::Deleted,
            _ => Self::Unspecified,
        }
    }

    pub fn as_proto(&self) -> i32 {
        match self {
            SessionLifecycleState::Unspecified => ProtoSessionLifecycleState::Unspecified as i32,
            SessionLifecycleState::Active => ProtoSessionLifecycleState::SessionLifecycleActive as i32,
            SessionLifecycleState::Suspended => ProtoSessionLifecycleState::SessionLifecycleSuspended as i32,
            SessionLifecycleState::Archived => ProtoSessionLifecycleState::SessionLifecycleArchived as i32,
            SessionLifecycleState::Deleted => ProtoSessionLifecycleState::SessionLifecycleDeleted as i32,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SessionLifecycleState::Unspecified => "unspecified",
            SessionLifecycleState::Active => "active",
            SessionLifecycleState::Suspended => "suspended",
            SessionLifecycleState::Archived => "archived",
            SessionLifecycleState::Deleted => "deleted",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionFilter {
    pub session_type: Option<String>,
    pub business_type: Option<String>,
    pub lifecycle_state: Option<SessionLifecycleState>,
    pub visibility: Option<SessionVisibility>,
    pub participant_user_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SessionSort {
    pub field: String,
    pub ascending: bool,
}

/// 会话领域配置值对象（只包含领域相关的配置）
#[derive(Clone, Debug)]
pub struct SessionDomainConfig {
    /// 最近消息限制（默认值）
    pub recent_message_limit: i32,
}

impl SessionDomainConfig {
    pub fn new(recent_message_limit: i32) -> Self {
        Self {
            recent_message_limit,
        }
    }

    pub fn default() -> Self {
        Self {
            recent_message_limit: 20,
        }
    }
}
