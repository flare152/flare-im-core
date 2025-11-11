use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use flare_proto::storage::Message;
use flare_storage_model::timestamp_to_datetime;
use serde_json::to_value;
use sqlx::{Pool, Postgres, postgres::PgPoolOptions};

use crate::domain::repositories::ArchiveStoreRepository;
use crate::infrastructure::config::StorageWriterConfig;

pub struct PostgresMessageStore {
    pool: Pool<Postgres>,
}

impl PostgresMessageStore {
    pub async fn new(config: &StorageWriterConfig) -> Result<Option<Self>> {
        let url = match &config.postgres_url {
            Some(url) => url,
            None => return Ok(None),
        };

        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;

        Ok(Some(Self { pool }))
    }
}

#[async_trait]
impl ArchiveStoreRepository for PostgresMessageStore {
    async fn store_archive(&self, message: &Message) -> Result<()> {
        let timestamp = message
            .timestamp
            .as_ref()
            .and_then(|ts| timestamp_to_datetime(ts))
            .unwrap_or_else(|| Utc::now());

        sqlx::query(
            r#"
            INSERT INTO messages (id, session_id, sender_id, receiver_ids, content, timestamp, extra)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (id) DO UPDATE
            SET session_id = EXCLUDED.session_id,
                sender_id = EXCLUDED.sender_id,
                receiver_ids = EXCLUDED.receiver_ids,
                content = EXCLUDED.content,
                timestamp = EXCLUDED.timestamp,
                extra = EXCLUDED.extra
            "#,
        )
        .bind(&message.id)
        .bind(&message.session_id)
        .bind(&message.sender_id)
        .bind(to_value(&message.receiver_ids)?)
        .bind(&message.content)
        .bind(timestamp)
        .bind(to_value(&message.extra)?)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
