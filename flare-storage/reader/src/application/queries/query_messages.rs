use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use chrono::{Duration, TimeZone, Utc};
use flare_im_core::utils::{TimelineMetadata, extract_timeline_from_extra, timestamp_to_datetime};
use flare_proto::common::Pagination;
use flare_proto::storage::{Message, QueryMessagesRequest, QueryMessagesResponse};

use crate::config::StorageReaderConfig;
use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

struct QueryCursor {
    ingestion_ts: i64,
    message_id: String,
}

impl QueryCursor {
    fn from_raw(raw: Option<&str>) -> Option<Self> {
        let raw = raw?;
        let mut parts = raw.splitn(2, ':');
        let ts = parts.next()?.parse::<i64>().ok()?;
        let message_id = parts.next()?.to_string();
        Some(Self {
            ingestion_ts: ts,
            message_id,
        })
    }
}

struct RetrievedMessage {
    message: Message,
    timeline: TimelineMetadata,
}

impl RetrievedMessage {
    fn new(message: Message, timeline: TimelineMetadata) -> Self {
        Self { message, timeline }
    }
}

pub struct QueryMessagesService {
    config: Arc<StorageReaderConfig>,
    storage: Arc<MongoMessageStorage>,
}

impl QueryMessagesService {
    pub async fn new(config: Arc<StorageReaderConfig>) -> Result<Self> {
        let storage = if let Some(url) = &config.mongo_url {
            MongoMessageStorage::new(url, &config.mongo_database)
                .await
                .unwrap_or_default()
        } else {
            MongoMessageStorage::default()
        };

        Ok(Self {
            config,
            storage: Arc::new(storage),
        })
    }

    pub async fn execute(&self, req: QueryMessagesRequest) -> Result<QueryMessagesResponse> {
        if req.session_id.is_empty() {
            return Err(anyhow!("session_id is required"));
        }

        let limit = req.limit.clamp(1, self.config.max_page_size) as usize;
        let cursor = QueryCursor::from_raw(if req.cursor.is_empty() {
            None
        } else {
            Some(req.cursor.as_str())
        });

        let end_ts = if req.end_time == 0 {
            Utc::now().timestamp()
        } else {
            req.end_time
        };
        let start_ts = if req.start_time == 0 {
            end_ts - self.config.default_range_seconds
        } else {
            req.start_time
        };

        let end_ts_ms = end_ts * 1_000;
        let start_ts_ms = start_ts * 1_000;

        // 计算总记录数（提前计算时间范围）
        let start_dt_for_count = Utc
            .timestamp_opt(start_ts, 0)
            .single()
            .unwrap_or_else(|| Utc::now() - Duration::seconds(self.config.default_range_seconds));
        let end_dt_for_count = Utc
            .timestamp_opt(end_ts, 0)
            .single()
            .unwrap_or_else(Utc::now);

        let total_size = self
            .storage
            .count_messages(&req.session_id, None, Some(start_dt_for_count), Some(end_dt_for_count))
            .await
            .unwrap_or(0);

        let mut seen = HashSet::new();
        if let Some(cursor) = &cursor {
            seen.insert(cursor.message_id.clone());
        }

        let mut aggregated = self
            .query_from_storage(
                &req.session_id,
                start_ts_ms,
                end_ts_ms,
                cursor.as_ref(),
                limit,
                &mut seen,
            )
            .await?;

        aggregated.sort_by(|a, b| b.timeline.ingestion_ts.cmp(&a.timeline.ingestion_ts));
        aggregated.truncate(limit);

        let messages: Vec<Message> = aggregated.iter().map(|item| item.message.clone()).collect();
        let next_cursor = if messages.len() == limit {
            aggregated
                .last()
                .map(|last| format!("{}:{}", last.timeline.ingestion_ts, last.message.id))
                .unwrap_or_default()
        } else {
            String::new()
        };

        Ok(QueryMessagesResponse {
            messages,
            next_cursor: next_cursor.clone(),
            has_more: !next_cursor.is_empty(),
            pagination: Some(Pagination {
                cursor: req.cursor.clone(),
                limit: limit as i32,
                has_more: !next_cursor.is_empty(),
                previous_cursor: String::new(),
                total_size,
            }),
            status: Some(flare_server_core::error::ok_status()),
        })
    }

    async fn query_from_storage(
        &self,
        session_id: &str,
        start_ts_ms: i64,
        end_ts_ms: i64,
        cursor: Option<&QueryCursor>,
        limit: usize,
        seen: &mut HashSet<String>,
    ) -> Result<Vec<RetrievedMessage>> {
        let start_dt = Utc
            .timestamp_millis_opt(start_ts_ms)
            .single()
            .unwrap_or_else(|| Utc::now() - Duration::days(30));
        let mut end_dt = Utc
            .timestamp_millis_opt(end_ts_ms)
            .single()
            .unwrap_or_else(Utc::now);

        if let Some(cursor) = cursor {
            if cursor.ingestion_ts <= start_ts_ms {
                return Ok(Vec::new());
            }
            end_dt = Utc
                .timestamp_millis_opt(cursor.ingestion_ts - 1)
                .single()
                .unwrap_or(end_dt);
        }

        if end_dt < start_dt {
            return Ok(Vec::new());
        }

        let messages = self
            .storage
            .query_messages(session_id, None, Some(start_dt), Some(end_dt), limit as i32)
            .await
            .map_err(|err| anyhow!(err.to_string()))?;

        let mut results = Vec::new();
        for message in messages {
            if !seen.insert(message.id.clone()) {
                continue;
            }

            let ingestion_hint = message
                .timestamp
                .as_ref()
                .and_then(timestamp_to_datetime)
                .map(|dt| dt.timestamp_millis())
                .unwrap_or_else(|| Utc::now().timestamp_millis());

            let timeline = extract_timeline_from_extra(&message.extra, ingestion_hint);
            results.push(RetrievedMessage::new(message, timeline));
            if results.len() >= limit {
                break;
            }
        }

        Ok(results)
    }
}
