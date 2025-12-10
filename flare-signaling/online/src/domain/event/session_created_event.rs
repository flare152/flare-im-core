use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::value_object::{DeviceId, DevicePriority, SessionId, TokenVersion, UserId};
use super::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreatedEvent {
    pub session_id: SessionId,
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub device_priority: DevicePriority,
    pub token_version: TokenVersion,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent for SessionCreatedEvent {
    fn event_type(&self) -> &'static str { "SessionCreated" }
    fn occurred_at(&self) -> DateTime<Utc> { self.occurred_at }
}
