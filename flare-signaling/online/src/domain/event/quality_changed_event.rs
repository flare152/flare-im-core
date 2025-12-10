use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::value_object::{ConnectionQuality, DeviceId, SessionId, UserId};
use super::DomainEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityChangedEvent {
    pub session_id: SessionId,
    pub user_id: UserId,
    pub device_id: DeviceId,
    pub old_quality: Option<ConnectionQuality>,
    pub new_quality: Option<ConnectionQuality>,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent for QualityChangedEvent {
    fn event_type(&self) -> &'static str { "QualityChanged" }
    fn occurred_at(&self) -> DateTime<Utc> { self.occurred_at }
}
