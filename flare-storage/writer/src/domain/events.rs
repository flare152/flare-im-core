use serde::Serialize;

#[derive(Serialize)]
pub struct AckEvent<'a> {
    pub message_id: &'a str,
    pub session_id: &'a str,
    pub status: AckStatus,
    pub ingestion_ts: i64,
    pub persisted_ts: i64,
    pub deduplicated: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AckStatus {
    Persisted,
    Duplicate,
}

impl AckStatus {
    pub fn from_deduplicated(deduplicated: bool) -> Self {
        if deduplicated {
            AckStatus::Duplicate
        } else {
            AckStatus::Persisted
        }
    }
}
