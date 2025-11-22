//! 工具函数模块
//!
//! 提供时间戳转换、时间线提取等通用工具函数

pub mod helpers;

pub use helpers::ServiceHelper;

use chrono::{DateTime, TimeZone, Utc};
use prost_types::Timestamp;
use serde_json;
use std::collections::HashMap;

/// 时间戳转换为毫秒数
pub fn timestamp_to_millis(ts: &Timestamp) -> Option<i64> {
    if ts.seconds == 0 && ts.nanos == 0 {
        return None;
    }
    Some(ts.seconds * 1000 + (ts.nanos as i64 / 1_000_000))
}

/// 毫秒数转换为时间戳
pub fn millis_to_timestamp(ms: i64) -> Option<Timestamp> {
    let seconds = ms / 1000;
    let nanos = ((ms % 1000) * 1_000_000) as i32;
    Some(Timestamp { seconds, nanos })
}

/// 时间戳转换为 DateTime
pub fn timestamp_to_datetime(ts: &Timestamp) -> Option<DateTime<Utc>> {
    timestamp_to_millis(ts).and_then(|ms| Utc.timestamp_millis_opt(ms).single())
}

/// DateTime 转换为时间戳
pub fn datetime_to_timestamp(dt: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    }
}

/// 获取当前时间戳（毫秒）
pub fn current_millis() -> i64 {
    Utc::now().timestamp_millis()
}

/// 时间线元数据
#[derive(Debug, Clone, Default)]
pub struct TimelineMetadata {
    pub emit_ts: Option<i64>,
    pub ingestion_ts: i64,
    pub persisted_ts: Option<i64>,
    pub dispatched_ts: Option<i64>,
    pub acked_ts: Option<i64>,
    pub read_ts: Option<i64>,
    pub deleted_ts: Option<i64>,
}

/// 从消息的 extra 字段中提取时间线元数据
pub fn extract_timeline_from_extra(
    extra: &HashMap<String, String>,
    default_ingestion_ts: i64,
) -> TimelineMetadata {
    if let Some(raw) = extra.get("timeline") {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(raw) {
            return TimelineMetadata {
                emit_ts: map.get("emit_ts").and_then(parse_i64),
                ingestion_ts: map
                    .get("ingestion_ts")
                    .and_then(parse_i64)
                    .unwrap_or(default_ingestion_ts),
                persisted_ts: map.get("persisted_ts").and_then(parse_i64),
                dispatched_ts: map.get("dispatched_ts").and_then(parse_i64),
                acked_ts: map.get("acked_ts").and_then(parse_i64),
                read_ts: map.get("read_ts").and_then(parse_i64),
                deleted_ts: map.get("deleted_ts").and_then(parse_i64),
            };
        }
    }

    TimelineMetadata {
        ingestion_ts: default_ingestion_ts,
        ..TimelineMetadata::default()
    }
}

/// 将时间线元数据嵌入到消息的 extra 字段中
pub fn embed_timeline_in_extra(
    message: &mut flare_proto::common::Message,
    timeline: &TimelineMetadata,
) {
    let mut timeline_map = HashMap::new();
    if let Some(value) = timeline.emit_ts {
        timeline_map.insert("emit_ts".to_string(), value.to_string());
    }
    timeline_map.insert(
        "ingestion_ts".to_string(),
        timeline.ingestion_ts.to_string(),
    );
    if let Some(value) = timeline.persisted_ts {
        timeline_map.insert("persisted_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.dispatched_ts {
        timeline_map.insert("dispatched_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.acked_ts {
        timeline_map.insert("acked_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.read_ts {
        timeline_map.insert("read_ts".to_string(), value.to_string());
    }
    if let Some(value) = timeline.deleted_ts {
        timeline_map.insert("deleted_ts".to_string(), value.to_string());
    }

    let json = serde_json::to_string(&timeline_map).unwrap_or_default();
    message.extra.insert("timeline".to_string(), json);
}

fn parse_i64(value: &String) -> Option<i64> {
    value.parse::<i64>().ok()
}

