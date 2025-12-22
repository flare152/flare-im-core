use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 会话记录（Connection Record）
///
/// 职责：表示一个设备的登录会话
/// 设计要点：
/// - 一个用户可以有多个会话（多设备登录）
/// - 会话ID作为设备在线的唯一标识
/// - 支持Token版本控制和链接质量监控
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub conversation_id: String,
    pub user_id: String,
    pub device_id: String,
    pub device_platform: String,
    pub server_id: String,
    pub gateway_id: String,
    pub last_seen: DateTime<Utc>,

    // 【新增】设备优先级（0=未指定, 1=低, 2=普通, 3=高, 4=关键）
    pub device_priority: i32,

    // 【新增】Token 版本（用于强制下线旧版本）
    pub token_version: i64,

    // 【新增】链接质量指标
    pub connection_quality: Option<ConnectionQualityRecord>,
}

/// 链接质量记录（Connection Quality Record）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionQualityRecord {
    pub rtt_ms: i64,           // 往返时延（毫秒）
    pub packet_loss_rate: f64, // 丢包率（0.0-1.0）
    pub last_measure_ts: i64,  // 最后测量时间戳（Unix 毫秒）
    pub network_type: String,  // 网络类型（wifi/4g/5g/ethernet）
    pub signal_strength: i32,  // 信号强度（dBm）
}
