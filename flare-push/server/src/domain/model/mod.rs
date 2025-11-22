use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PushDispatchTask {
    pub user_id: String,
    #[serde(default)]
    pub message_id: String,
    #[serde(default)]
    pub message_type: String,
    pub message: Vec<u8>,
    pub notification: Option<DispatchNotification>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
    pub online: bool,
    pub tenant_id: Option<String>,
    pub require_online: bool,
    pub persist_if_offline: bool,
    pub priority: i32,
    pub context: Option<RequestMetadata>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DispatchNotification {
    pub title: String,
    pub body: String,
    pub data: HashMap<String, String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestMetadata {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}
