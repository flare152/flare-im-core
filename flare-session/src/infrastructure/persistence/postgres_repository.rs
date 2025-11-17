//! # PostgreSQL Session Repository
//!
//! PostgreSQL持久化层实现，用于会话元数据存储

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use tracing::info;

use crate::config::SessionConfig;
use crate::domain::models::{
    Session, SessionBootstrapResult, SessionFilter, SessionParticipant,
    SessionSort, SessionSummary,
};
use crate::domain::repositories::SessionRepository;

/// 会话查询行结构（用于SQL查询结果映射）
#[derive(sqlx::FromRow)]
#[allow(dead_code)]
struct SessionRow {
    session_id: String,
    session_type: String,
    business_type: String,
    display_name: Option<String>,
    attributes: serde_json::Value,
    visibility: String,
    lifecycle_state: String,
    updated_at: DateTime<Utc>,
}

/// PostgreSQL Session Repository实现
pub struct PostgresSessionRepository {
    pool: Arc<PgPool>,
    config: Arc<SessionConfig>,
}

impl PostgresSessionRepository {
    /// 创建PostgreSQL Session Repository
    pub fn new(pool: Arc<PgPool>, config: Arc<SessionConfig>) -> Self {
        Self { pool, config }
    }
}

#[async_trait]
impl SessionRepository for PostgresSessionRepository {
    async fn load_bootstrap(
        &self,
        user_id: &str,
        client_cursor: &HashMap<String, i64>,
    ) -> Result<SessionBootstrapResult> {
        // 1. 从user_sync_cursor表加载用户的光标映射
        let cursor_rows = sqlx::query(
            r#"
            SELECT session_id, last_synced_ts
            FROM user_sync_cursor
            WHERE user_id = $1
            "#,
        )
        .bind(user_id)
        .fetch_all(&*self.pool)
        .await
        .context("Failed to load user cursors")?;

        let mut server_cursor: HashMap<String, i64> = cursor_rows
            .into_iter()
            .map(|row| {
                let session_id: String = row.get("session_id");
                let ts: i64 = row.get("last_synced_ts");
                (session_id, ts)
            })
            .collect();

        // merge client cursor hints to ensure we cover requested sessions
        for (session_id, ts) in client_cursor {
            server_cursor.entry(session_id.clone()).or_insert(*ts);
        }

        // 2. 从sessions和session_participants表查询用户参与的会话
        let session_rows = sqlx::query(
            r#"
            SELECT DISTINCT
                s.session_id,
                s.session_type,
                s.business_type,
                s.display_name,
                s.attributes,
                s.visibility,
                s.lifecycle_state,
                s.updated_at
            FROM sessions s
            INNER JOIN session_participants sp ON s.session_id = sp.session_id
            WHERE sp.user_id = $1
              AND s.lifecycle_state != 'deleted'
            ORDER BY s.updated_at DESC
            "#,
        )
        .bind(user_id)
        .fetch_all(&*self.pool)
        .await
        .context("Failed to load user sessions")?;

        let mut summaries = Vec::new();

        for row in session_rows {
            let session_id: String = row.get("session_id");
            let session_type: Option<String> = row.get("session_type");
            let business_type: Option<String> = row.get("business_type");
            let display_name: Option<String> = row.get("display_name");
            let attributes: serde_json::Value = row.get("attributes");
            let updated_at: DateTime<Utc> = row.get("updated_at");

            let attributes: HashMap<String, String> = serde_json::from_value(attributes)
                .unwrap_or_default();

            // 注释：最后一条消息信息将在ApplicationService层通过MessageProvider补充
            // 当前实现使用updated_at作为server_cursor_ts的fallback
            let server_cursor_ts = server_cursor
                .get(&session_id)
                .copied()
                .or_else(|| Some(updated_at.timestamp_millis()));

            // 计算未读数：基于server_cursor_ts和用户游标的差值
            // 简化实现：如果有server_cursor_ts且大于用户游标，则认为可能有未读消息
            // 实际未读数将在ApplicationService层通过MessageProvider精确计算
            let user_cursor_ts = server_cursor.get(&session_id).copied().unwrap_or(0);
            let server_ts = server_cursor_ts.unwrap_or(0);
            let estimated_unread = if server_ts > user_cursor_ts {
                // 基于时间差估算，但实际值需要从消息存储服务查询
                // 这里先设为0，将在ApplicationService层补充
                0
            } else {
                0
            };

            let summary = SessionSummary {
                session_id,
                session_type,
                business_type,
                last_message_id: None, // 将在ApplicationService层补充
                last_message_time: None, // 将在ApplicationService层补充
                last_sender_id: None, // 将在ApplicationService层补充
                last_message_type: None, // 将在ApplicationService层补充
                last_content_type: None, // 将在ApplicationService层补充
                unread_count: estimated_unread, // 将在ApplicationService层精确计算
                metadata: attributes,
                server_cursor_ts,
                display_name,
            };

            summaries.push(summary);
        }

        // 按server_cursor_ts降序排序
        summaries.sort_by(|a, b| {
            let at = a.server_cursor_ts.unwrap_or_default();
            let bt = b.server_cursor_ts.unwrap_or_default();
            bt.cmp(&at)
        });

        Ok(SessionBootstrapResult {
            summaries,
            recent_messages: Vec::new(),
            cursor_map: server_cursor,
            policy: self.config.default_policy.clone(),
        })
    }

    async fn update_cursor(&self, user_id: &str, session_id: &str, ts: i64) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO user_sync_cursor (user_id, session_id, last_synced_ts, updated_at)
            VALUES ($1, $2, $3, CURRENT_TIMESTAMP)
            ON CONFLICT (user_id, session_id)
            DO UPDATE SET last_synced_ts = $3, updated_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(user_id)
        .bind(session_id)
        .bind(ts)
        .execute(&*self.pool)
        .await
        .context("Failed to update cursor")?;

        Ok(())
    }

    async fn create_session(&self, session: &Session) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // 插入会话记录
        sqlx::query(
            r#"
            INSERT INTO sessions (
                session_id, session_type, business_type, display_name,
                attributes, visibility, lifecycle_state, metadata, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#,
        )
        .bind(&session.session_id)
        .bind(&session.session_type)
        .bind(&session.business_type)
        .bind(&session.display_name)
        .bind(serde_json::to_value(&session.attributes)?)
        .bind(session.visibility.as_str())
        .bind(session.lifecycle_state.as_str())
        .bind(serde_json::to_value(&HashMap::<String, String>::new())?)
        .execute(&mut *tx)
        .await
        .context("Failed to create session")?;

        // 插入参与者记录
        for participant in &session.participants {
            sqlx::query(
                r#"
                INSERT INTO session_participants (
                    session_id, user_id, roles, muted, pinned, attributes, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                "#,
            )
            .bind(&session.session_id)
            .bind(&participant.user_id)
            .bind(&participant.roles)
            .bind(participant.muted)
            .bind(participant.pinned)
            .bind(serde_json::to_value(&participant.attributes)?)
            .execute(&mut *tx)
            .await
            .context("Failed to create participant")?;
        }

        tx.commit().await?;
        info!(session_id = %session.session_id, "Session created");
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        let row = sqlx::query(
            r#"
            SELECT session_id, session_type, business_type, display_name,
                   attributes, visibility, lifecycle_state, metadata,
                   created_at, updated_at
            FROM sessions
            WHERE session_id = $1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&*self.pool)
        .await
        .context("Failed to get session")?;

        let Some(row) = row else {
            return Ok(None);
        };

        let session_id: String = row.get("session_id");
        let session_type: String = row.get("session_type");
        let business_type: String = row.get("business_type");
        let display_name: Option<String> = row.get("display_name");
        let attributes: serde_json::Value = row.get("attributes");
        let visibility: String = row.get("visibility");
        let lifecycle_state: String = row.get("lifecycle_state");
        let created_at: DateTime<Utc> = row.get("created_at");
        let updated_at: DateTime<Utc> = row.get("updated_at");

        let attributes: HashMap<String, String> = serde_json::from_value(attributes)
            .unwrap_or_default();

        let visibility = match visibility.as_str() {
            "private" => crate::domain::models::SessionVisibility::Private,
            "tenant" => crate::domain::models::SessionVisibility::Tenant,
            "public" => crate::domain::models::SessionVisibility::Public,
            _ => crate::domain::models::SessionVisibility::Unspecified,
        };

        let lifecycle_state = match lifecycle_state.as_str() {
            "active" => crate::domain::models::SessionLifecycleState::Active,
            "suspended" => crate::domain::models::SessionLifecycleState::Suspended,
            "archived" => crate::domain::models::SessionLifecycleState::Archived,
            "deleted" => crate::domain::models::SessionLifecycleState::Deleted,
            _ => crate::domain::models::SessionLifecycleState::Unspecified,
        };

        // 查询参与者
        let participant_rows = sqlx::query(
            r#"
            SELECT user_id, roles, muted, pinned, attributes
            FROM session_participants
            WHERE session_id = $1
            "#,
        )
        .bind(&session_id)
        .fetch_all(&*self.pool)
        .await
        .context("Failed to get participants")?;

        let mut participants = Vec::new();
        for p_row in participant_rows {
            let user_id: String = p_row.get("user_id");
            let roles: Vec<String> = p_row.get("roles");
            let muted: bool = p_row.get("muted");
            let pinned: bool = p_row.get("pinned");
            let attributes: serde_json::Value = p_row.get("attributes");
            let attributes: HashMap<String, String> = serde_json::from_value(attributes)
                .unwrap_or_default();

            participants.push(SessionParticipant {
                user_id,
                roles,
                muted,
                pinned,
                attributes,
            });
        }

        Ok(Some(Session {
            session_id,
            session_type,
            business_type,
            display_name,
            attributes,
            participants,
            visibility,
            lifecycle_state,
            policy: None,
            created_at,
            updated_at,
        }))
    }

    async fn update_session(&self, session: &Session) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE sessions
            SET display_name = $1,
                attributes = $2,
                visibility = $3,
                lifecycle_state = $4,
                updated_at = CURRENT_TIMESTAMP
            WHERE session_id = $5
            "#,
        )
        .bind(&session.display_name)
        .bind(serde_json::to_value(&session.attributes)?)
        .bind(session.visibility.as_str())
        .bind(session.lifecycle_state.as_str())
        .bind(&session.session_id)
        .execute(&*self.pool)
        .await
        .context("Failed to update session")?;

        info!(session_id = %session.session_id, "Session updated");
        Ok(())
    }

    async fn delete_session(&self, session_id: &str, hard_delete: bool) -> Result<()> {
        if hard_delete {
            // 物理删除（级联删除参与者）
            sqlx::query("DELETE FROM sessions WHERE session_id = $1")
                .bind(session_id)
                .execute(&*self.pool)
                .await
                .context("Failed to delete session")?;
        } else {
            // 软删除（更新生命周期状态）
            sqlx::query(
                r#"
                UPDATE sessions
                SET lifecycle_state = 'deleted', updated_at = CURRENT_TIMESTAMP
                WHERE session_id = $1
                "#,
            )
            .bind(session_id)
            .execute(&*self.pool)
            .await
            .context("Failed to delete session")?;
        }

        info!(session_id = %session_id, hard_delete = hard_delete, "Session deleted");
        Ok(())
    }

    async fn manage_participants(
        &self,
        session_id: &str,
        to_add: &[SessionParticipant],
        to_remove: &[String],
        role_updates: &[(String, Vec<String>)],
    ) -> Result<Vec<SessionParticipant>> {
        let mut tx = self.pool.begin().await?;

        // 添加参与者
        for participant in to_add {
            sqlx::query(
                r#"
                INSERT INTO session_participants (
                    session_id, user_id, roles, muted, pinned, attributes, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                ON CONFLICT (session_id, user_id)
                DO UPDATE SET
                    roles = $3,
                    muted = $4,
                    pinned = $5,
                    attributes = $6,
                    updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(session_id)
            .bind(&participant.user_id)
            .bind(&participant.roles)
            .bind(participant.muted)
            .bind(participant.pinned)
            .bind(serde_json::to_value(&participant.attributes)?)
            .execute(&mut *tx)
            .await
            .context("Failed to add participant")?;
        }

        // 删除参与者
        for user_id in to_remove {
            sqlx::query(
                "DELETE FROM session_participants WHERE session_id = $1 AND user_id = $2",
            )
            .bind(session_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Failed to remove participant")?;
        }

        // 更新角色
        for (user_id, roles) in role_updates {
            sqlx::query(
                r#"
                UPDATE session_participants
                SET roles = $1, updated_at = CURRENT_TIMESTAMP
                WHERE session_id = $2 AND user_id = $3
                "#,
            )
            .bind(roles)
            .bind(session_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .context("Failed to update participant roles")?;
        }

        tx.commit().await?;

        // 返回更新后的参与者列表
        let participant_rows = sqlx::query(
            r#"
            SELECT user_id, roles, muted, pinned, attributes
            FROM session_participants
            WHERE session_id = $1
            "#,
        )
        .bind(session_id)
        .fetch_all(&*self.pool)
        .await
        .context("Failed to get participants")?;

        let mut participants = Vec::new();
        for p_row in participant_rows {
            let user_id: String = p_row.get("user_id");
            let roles: Vec<String> = p_row.get("roles");
            let muted: bool = p_row.get("muted");
            let pinned: bool = p_row.get("pinned");
            let attributes: serde_json::Value = p_row.get("attributes");
            let attributes: HashMap<String, String> = serde_json::from_value(attributes)
                .unwrap_or_default();

            participants.push(SessionParticipant {
                user_id,
                roles,
                muted,
                pinned,
                attributes,
            });
        }

        Ok(participants)
    }

    async fn batch_acknowledge(
        &self,
        user_id: &str,
        cursors: &[(String, i64)],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        for (session_id, ts) in cursors {
            sqlx::query(
                r#"
                INSERT INTO user_sync_cursor (user_id, session_id, last_synced_ts, updated_at)
                VALUES ($1, $2, $3, CURRENT_TIMESTAMP)
                ON CONFLICT (user_id, session_id)
                DO UPDATE SET last_synced_ts = $3, updated_at = CURRENT_TIMESTAMP
                "#,
            )
            .bind(user_id)
            .bind(session_id)
            .bind(*ts)
            .execute(&mut *tx)
            .await
            .context("Failed to acknowledge cursor")?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn search_sessions(
        &self,
        user_id: Option<&str>,
        filters: &[SessionFilter],
        sort: &[SessionSort],
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SessionSummary>, usize)> {
        // 构建基础查询
        let mut query = String::from(
            r#"
            SELECT DISTINCT
                s.session_id,
                s.session_type,
                s.business_type,
                s.display_name,
                s.attributes,
                s.visibility,
                s.lifecycle_state,
                s.updated_at
            FROM sessions s
            "#,
        );

        // 如果指定了user_id，需要JOIN session_participants表
        if user_id.is_some() {
            query.push_str("INNER JOIN session_participants sp ON s.session_id = sp.session_id\n");
        }

        // 构建WHERE子句
        let mut conditions = Vec::new();
        let mut bind_index = 1;

        if user_id.is_some() {
            conditions.push(format!("sp.user_id = ${}", bind_index));
            bind_index += 1;
        }

        // 应用过滤器
        for filter in filters {
            if filter.session_type.is_some() {
                conditions.push(format!("s.session_type = ${}", bind_index));
                bind_index += 1;
            }
            if filter.business_type.is_some() {
                conditions.push(format!("s.business_type = ${}", bind_index));
                bind_index += 1;
            }
            if filter.lifecycle_state.is_some() {
                conditions.push(format!("s.lifecycle_state = ${}", bind_index));
                bind_index += 1;
            }
            if filter.visibility.is_some() {
                conditions.push(format!("s.visibility = ${}", bind_index));
                bind_index += 1;
            }
            if filter.participant_user_id.is_some() {
                if !query.contains("session_participants") {
                    query.push_str("INNER JOIN session_participants sp2 ON s.session_id = sp2.session_id\n");
                }
                conditions.push(format!("sp2.user_id = ${}", bind_index));
                bind_index += 1;
            }
        }

        // 默认过滤：排除已删除的会话
        conditions.push("s.lifecycle_state != 'deleted'".to_string());

        if !conditions.is_empty() {
            query.push_str("WHERE ");
            query.push_str(&conditions.join(" AND "));
        }

        // 构建ORDER BY子句
        if sort.is_empty() {
            query.push_str(" ORDER BY s.updated_at DESC");
        } else {
            let mut order_clauses = Vec::new();
            for s in sort {
                let direction = if s.ascending { "ASC" } else { "DESC" };
                let field = match s.field.as_str() {
                    "created_at" => "s.created_at",
                    "updated_at" => "s.updated_at",
                    "session_type" => "s.session_type",
                    "business_type" => "s.business_type",
                    _ => "s.updated_at", // 默认字段
                };
                order_clauses.push(format!("{} {}", field, direction));
            }
            query.push_str(" ORDER BY ");
            query.push_str(&order_clauses.join(", "));
        }

        // 添加LIMIT和OFFSET
        query.push_str(&format!(" LIMIT ${} OFFSET ${}", bind_index, bind_index + 1));

        // 执行查询（使用query而不是query_as，因为动态SQL构建）
        let mut query_builder = sqlx::query(&query);

        if let Some(uid) = user_id {
            query_builder = query_builder.bind(uid);
        }

        // 绑定过滤器参数
        for filter in filters {
            if let Some(ref st) = filter.session_type {
                query_builder = query_builder.bind(st);
            }
            if let Some(ref bt) = filter.business_type {
                query_builder = query_builder.bind(bt);
            }
            if let Some(ref ls) = filter.lifecycle_state {
                query_builder = query_builder.bind(ls.as_str());
            }
            if let Some(ref vis) = filter.visibility {
                query_builder = query_builder.bind(vis.as_str());
            }
            if let Some(ref pid) = filter.participant_user_id {
                query_builder = query_builder.bind(pid);
            }
        }

        query_builder = query_builder.bind(limit as i64).bind(offset as i64);

        let rows = query_builder
            .fetch_all(&*self.pool)
            .await
            .context("Failed to search sessions")?;

        // 转换为SessionSummary
        let summaries: Vec<SessionSummary> = rows
            .into_iter()
            .map(|row| {
                let session_id: String = row.get("session_id");
                let session_type: String = row.get("session_type");
                let business_type: String = row.get("business_type");
                let display_name: Option<String> = row.get("display_name");
                let attributes: serde_json::Value = row.get("attributes");
                let updated_at: DateTime<Utc> = row.get("updated_at");

                let attributes: HashMap<String, String> =
                    serde_json::from_value(attributes).unwrap_or_default();
                let server_cursor_ts = Some(updated_at.timestamp_millis());

                SessionSummary {
                    session_id,
                    session_type: Some(session_type),
                    business_type: Some(business_type),
                    last_message_id: None,
                    last_message_time: None,
                    last_sender_id: None,
                    last_message_type: None,
                    last_content_type: None,
                    unread_count: 0, // 将在ApplicationService层通过MessageProvider精确计算
                    metadata: attributes,
                    server_cursor_ts,
                    display_name,
                }
            })
            .collect();

        // 查询总数（用于分页）
        // 注意：总数查询可能较慢，生产环境建议：
        // 1. 使用Redis缓存查询结果（TTL 5-10分钟）
        // 2. 使用近似值（如通过采样估算）
        // 3. 对于大用户，考虑使用分页而不显示总数
        let count_query = query.replace("SELECT DISTINCT", "SELECT COUNT(DISTINCT s.session_id)");
        let count_query = count_query.split("LIMIT").next().unwrap_or(&count_query);
        let mut count_builder = sqlx::query_scalar::<_, i64>(count_query);

        if let Some(uid) = user_id {
            count_builder = count_builder.bind(uid);
        }

        // 绑定过滤器参数（与上面相同）
        for filter in filters {
            if let Some(ref st) = filter.session_type {
                count_builder = count_builder.bind(st);
            }
            if let Some(ref bt) = filter.business_type {
                count_builder = count_builder.bind(bt);
            }
            if let Some(ref ls) = filter.lifecycle_state {
                count_builder = count_builder.bind(ls.as_str());
            }
            if let Some(ref vis) = filter.visibility {
                count_builder = count_builder.bind(vis.as_str());
            }
            if let Some(ref pid) = filter.participant_user_id {
                count_builder = count_builder.bind(pid);
            }
        }

        let total = count_builder
            .fetch_one(&*self.pool)
            .await
            .unwrap_or(0) as usize;

        Ok((summaries, total))
    }
}

