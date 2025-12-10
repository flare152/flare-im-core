use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::value_object::{DeviceId, SessionId, UserId};
use super::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionKickedEvent {
    pub session_id: SessionId,
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub reason: String,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent for SessionKickedEvent {
    fn event_type(&self) -> &'static str { "SessionKicked" }
    fn occurred_at(&self) -> DateTime<Utc> { self.occurred_at }
}
