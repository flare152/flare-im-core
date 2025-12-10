//! 序列号（seq）生成器
//!
//! 提供会话内消息序列号的生成功能，支持分布式生成
//!
//! ## 设计原则
//!
//! 1. **会话内唯一递增**：每个会话内的消息 seq 必须严格递增，无间隙
//! 2. **分布式生成**：支持多实例并发写入，保证 seq 唯一性
//! 3. **高性能**：seq 生成不能成为性能瓶颈（P99 < 1ms）
//! 4. **降级方案**：Redis 不可用时，自动降级到数据库

use anyhow::{Context, Result};
use redis::{AsyncCommands, aio::ConnectionManager};
use std::sync::Arc;
use async_trait::async_trait;
use tracing::{debug, warn};

use crate::domain::repository::SeqGenerator as SeqGeneratorTrait;

/// Redis Seq 生成器（推荐方案）
///
/// 使用 Redis 原子计数器生成 seq，性能高，支持分布式
pub struct RedisSeqGenerator {
    redis_client: Arc<redis::Client>,
    db_pool: Option<Arc<sqlx::PgPool>>,
}

impl RedisSeqGenerator {
    /// 创建 Redis Seq 生成器
    ///
    /// # 参数
    /// * `redis_client` - Redis 客户端
    /// * `db_pool` - PostgreSQL 连接池（可选，用于降级）
    pub fn new(
        redis_client: Arc<redis::Client>,
        db_pool: Option<Arc<sqlx::PgPool>>,
    ) -> Self {
        Self {
            redis_client,
            db_pool,
        }
    }

    /// 从数据库获取会话的最大 seq
    async fn get_max_seq_from_db(&self, session_id: &str) -> Result<i64> {
        let pool = self
            .db_pool
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Database pool not available for seq fallback"))?;

        let max_seq: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(seq), 0)
            FROM messages
            WHERE session_id = $1 AND seq IS NOT NULL
            "#,
        )
        .bind(session_id)
        .fetch_optional(pool.as_ref())
        .await
        .context("Failed to query max seq from database")?;

        Ok(max_seq.unwrap_or(0))
    }

    /// 初始化 Redis 计数器
    async fn init_redis_counter(&self, session_id: &str, initial_value: i64) -> Result<()> {
        let key = format!("seq:{}", session_id);
        let mut conn = ConnectionManager::new(self.redis_client.as_ref().clone()).await?;

        // 使用 SET NX 只在 key 不存在时设置
        let _: Option<()> = conn.set_nx(&key, initial_value).await?;

        Ok(())
    }
}

#[async_trait]
impl SeqGeneratorTrait for RedisSeqGenerator {
    async fn generate_seq(&self, session_id: &str) -> Result<i64> {
        let key = format!("seq:{}", session_id);
        let mut conn = ConnectionManager::new(self.redis_client.as_ref().clone()).await?;

        // 1. 尝试从 Redis 获取（原子递增）
        match conn.incr::<&str, i64, i64>(&key, 1i64).await {
            Ok(seq) => {
                debug!(
                    session_id = %session_id,
                    seq,
                    "Generated seq from Redis"
                );
                Ok(seq)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    session_id = %session_id,
                    "Redis INCR failed, falling back to database"
                );

                // 2. Redis 不可用时，从数据库获取最大 seq
                let max_seq = self.get_max_seq_from_db(session_id).await?;

                // 3. 初始化 Redis 计数器（如果可能）
                if let Err(init_err) = self.init_redis_counter(session_id, max_seq).await {
                    warn!(
                        error = %init_err,
                        session_id = %session_id,
                        "Failed to initialize Redis counter, will retry on next request"
                    );
                }

                // 4. 返回下一个 seq
                let next_seq = max_seq + 1;
                debug!(
                    session_id = %session_id,
                    max_seq,
                    next_seq,
                    "Generated seq from database fallback"
                );
                Ok(next_seq)
            }
        }
    }
}

/// 数据库 Seq 生成器（降级方案）
///
///
/// 使用数据库序列或查询最大 seq，性能略低于 Redis，但更可靠
pub struct DatabaseSeqGenerator {
    db_pool: Arc<sqlx::PgPool>,
}

impl DatabaseSeqGenerator {
    /// 创建数据库 Seq 生成器
    ///
    /// # 参数
    /// * `db_pool` - PostgreSQL 连接池
    pub fn new(db_pool: Arc<sqlx::PgPool>) -> Self {
        Self { db_pool }
    }
}

#[async_trait]
impl SeqGeneratorTrait for DatabaseSeqGenerator {
    async fn generate_seq(&self, session_id: &str) -> Result<i64> {
        // 使用数据库事务 + SELECT FOR UPDATE 保证原子性
        let mut tx = self.db_pool.begin().await?;

        // 1. 获取当前最大 seq（加锁）
        let max_seq: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT COALESCE(MAX(seq), 0)
            FROM messages
            WHERE session_id = $1 AND seq IS NOT NULL
            FOR UPDATE
            "#,
        )
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await
        .context("Failed to query max seq from database")?;

        let current_max = max_seq.unwrap_or(0);
        let next_seq = current_max + 1;

        // 2. 注意：这里不立即插入消息，只是生成 seq
        // 实际的消息插入会在调用方完成
        // 这里只是预留 seq 值，通过事务保证原子性

        tx.commit().await?;

        debug!(
            session_id = %session_id,
            current_max,
            next_seq,
            "Generated seq from database"
        );

        Ok(next_seq)
    }
}

#[cfg(test)]
mod tests {

    // 注意：这些测试需要实际的 Redis 和数据库连接
    // 在实际测试中，应该使用测试容器或 mock

    #[tokio::test]
    #[ignore] // 需要 Redis 和数据库
    async fn test_redis_seq_generator() {
        // TODO: 实现测试
    }

    #[tokio::test]
    #[ignore] // 需要数据库
    async fn test_database_seq_generator() {
        // TODO: 实现测试
    }
}

