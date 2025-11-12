use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DispatchNotification {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub data: HashMap<String, String>,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RequestMetadata {
    pub request_id: String,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub client_ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
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
