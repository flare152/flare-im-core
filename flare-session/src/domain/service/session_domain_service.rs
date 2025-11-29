//! 会话领域服务 - 包含所有业务逻辑实现

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use flare_proto::common::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;
use flare_core::common::session_id::{generate_server_session_id, SessionType, validate_session_id};

use crate::domain::model::{
    ConflictResolutionPolicy, DevicePresence, DeviceState, MessageSyncResult, Session,
    SessionDomainConfig, SessionFilter, SessionLifecycleState, SessionParticipant,
    SessionPolicy, SessionSort, SessionSummary, SessionVisibility,
};
use crate::domain::repository::{
    MessageProvider, PresenceRepository, PresenceUpdate, SessionRepository,
};

/// 会话领域服务 - 包含所有业务逻辑
pub struct SessionDomainService {
    session_repo: Arc<dyn SessionRepository>,
    presence_repo: Arc<dyn PresenceRepository>,
    message_provider: Option<Arc<dyn MessageProvider>>,
    config: SessionDomainConfig,
}

/// 会话引导输出
pub struct SessionBootstrapOutput {
    pub summaries: Vec<SessionSummary>,
    pub recent_messages: Vec<Message>,
    pub cursor_map: HashMap<String, i64>,
    pub devices: Vec<DevicePresence>,
    pub policy: SessionPolicy,
}

impl SessionDomainService {
    pub fn new(
        session_repo: Arc<dyn SessionRepository>,
        presence_repo: Arc<dyn PresenceRepository>,
        message_provider: Option<Arc<dyn MessageProvider>>,
        config: SessionDomainConfig,
    ) -> Self {
        Self {
            session_repo,
            presence_repo,
            message_provider,
            config,
        }
    }

    /// 会话引导（业务逻辑）
    pub async fn bootstrap_session(
        &self,
        user_id: &str,
        client_cursor: HashMap<String, i64>,
        include_recent: bool,
        recent_limit: Option<i32>,
    ) -> Result<SessionBootstrapOutput> {
        let bootstrap = self
            .session_repo
            .load_bootstrap(user_id, &client_cursor)
            .await?;

        let mut summaries = bootstrap.summaries;
        
        // 如果有消息提供者，补充最后一条消息信息和未读数
        if let Some(provider) = &self.message_provider {
            // 为每个会话获取最后一条消息（如果有）
            for summary in &mut summaries {
                if summary.last_message_id.is_none() {
                    // 尝试获取最后一条消息信息
                    if let Ok(sync_result) = provider
                        .sync_messages(&summary.session_id, 0, None, 1)
                        .await
                    {
                        if let Some(last_msg) = sync_result.messages.first() {
                            summary.last_message_id = Some(last_msg.id.clone());
                            
                            // 转换 Timestamp 为 DateTime<Utc>
                            summary.last_message_time = last_msg.timestamp.as_ref().and_then(|ts| {
                                chrono::TimeZone::timestamp_opt(
                                    &chrono::Utc,
                                    ts.seconds,
                                    ts.nanos as u32,
                                )
                                .single()
                            });
                            
                            summary.last_sender_id = Some(last_msg.sender_id.clone());
                            summary.last_message_type = Some(last_msg.message_type() as i32);
                            
                            // 从消息内容推断内容类型
                            if let Some(ref content) = last_msg.content {
                                summary.last_content_type = match &content.content {
                                    Some(flare_proto::common::message_content::Content::Text(_)) => {
                                        Some("text".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Image(_)) => {
                                        Some("image".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Video(_)) => {
                                        Some("video".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Audio(_)) => {
                                        Some("audio".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::File(_)) => {
                                        Some("file".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Location(_)) => {
                                        Some("location".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Card(_)) => {
                                        Some("card".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Notification(_)) => {
                                        Some("notification".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Custom(_)) => {
                                        Some("custom".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Forward(_)) => {
                                        Some("forward".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Typing(_)) => {
                                        Some("typing".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Vote(_)) => {
                                        Some("vote".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Task(_)) => {
                                        Some("task".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Schedule(_)) => {
                                        Some("schedule".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::Announcement(_)) => {
                                        Some("announcement".to_string())
                                    }
                                    Some(flare_proto::common::message_content::Content::SystemEvent(_)) => {
                                        Some("system_event".to_string())
                                    }
                                    None => None,
                                };
                            }
                            
                            // 更新server_cursor_ts为最后消息的时间戳
                            if let Some(ts) = last_msg.timestamp.as_ref() {
                                summary.server_cursor_ts = Some(ts.seconds * 1_000 + (ts.nanos as i64 / 1_000_000));
                            }
                        }
                    }
                    
                    // 未读数已在 load_bootstrap 中从数据库读取（基于 seq）
                    // 这里不再需要重新计算
                }
            }
        }

        let mut recent_messages = Vec::new();
        if include_recent {
            if let Some(provider) = &self.message_provider {
                let session_ids: Vec<String> = summaries
                    .iter()
                    .map(|s| s.session_id.clone())
                    .collect();
                if !session_ids.is_empty() {
                    recent_messages = provider
                        .recent_messages(
                            &session_ids,
                            recent_limit.unwrap_or(self.config.recent_message_limit),
                            &bootstrap.cursor_map,
                        )
                        .await
                        .unwrap_or_default();
                }
            }
        }

        let devices = self
            .presence_repo
            .list_devices(user_id)
            .await
            .unwrap_or_default();

        Ok(SessionBootstrapOutput {
            summaries,
            recent_messages,
            cursor_map: bootstrap.cursor_map,
            devices,
            policy: bootstrap.policy,
        })
    }

    /// 列出会话（业务逻辑）
    pub async fn list_sessions(
        &self,
        user_id: &str,
        cursor: Option<&str>,
        limit: i32,
    ) -> Result<(Vec<SessionSummary>, Option<String>, bool)> {
        let bootstrap = self
            .session_repo
            .load_bootstrap(user_id, &HashMap::new())
            .await?;

        let mut summaries = bootstrap.summaries;
        let (pivot_ts, pivot_id) = parse_cursor(cursor);

        if let Some(ts) = pivot_ts {
            summaries.retain(|summary| match summary.server_cursor_ts {
                Some(summary_ts) if summary_ts < ts => true,
                Some(summary_ts) if summary_ts == ts => summary.session_id > pivot_id,
                Some(_) => false,
                None => false,
            });
        }

        let limit = limit.max(1) as usize;
        let has_more = summaries.len() > limit;
        summaries.truncate(limit);

        let next_cursor = summaries.last().and_then(|summary| {
            summary
                .server_cursor_ts
                .map(|ts| format!("{}:{}", ts, summary.session_id))
        });

        Ok((summaries, next_cursor, has_more))
    }

    /// 同步消息（业务逻辑）
    pub async fn sync_messages(
        &self,
        session_id: &str,
        since_ts: i64,
        cursor: Option<&str>,
        limit: i32,
    ) -> Result<MessageSyncResult> {
        let provider = self
            .message_provider
            .as_ref()
            .ok_or_else(|| anyhow!("message provider not configured"))?;
        provider
            .sync_messages(session_id, since_ts, cursor, limit)
            .await
    }

    /// 更新游标（业务逻辑）
    pub async fn update_cursor(
        &self,
        user_id: &str,
        session_id: &str,
        message_ts: i64,
    ) -> Result<()> {
        self.session_repo
            .update_cursor(user_id, session_id, message_ts)
            .await
    }

    /// 更新设备状态（业务逻辑）
    pub async fn update_presence(
        &self,
        user_id: &str,
        device_id: &str,
        platform: Option<String>,
        state: DeviceState,
        conflict_resolution: Option<ConflictResolutionPolicy>,
        notify_conflict: bool,
        conflict_reason: Option<String>,
    ) -> Result<()> {
        let update = PresenceUpdate {
            user_id: user_id.to_string(),
            device_id: device_id.to_string(),
            device_platform: platform,
            state,
            conflict_resolution,
            notify_conflict,
            conflict_reason,
        };
        self.presence_repo.update_presence(update).await
    }

    /// 强制会话同步（业务逻辑）
    pub async fn force_session_sync(
        &self,
        user_id: &str,
        session_ids: &[String],
        reason: Option<&str>,
    ) -> Result<Vec<String>> {
        if session_ids.is_empty() {
            return Ok(Vec::new());
        }

        let bootstrap = self
            .session_repo
            .load_bootstrap(user_id, &HashMap::new())
            .await?;

        let known: HashSet<String> = bootstrap
            .summaries
            .iter()
            .map(|summary| summary.session_id.clone())
            .collect();

        let missing: Vec<String> = session_ids
            .iter()
            .filter(|session_id| !known.contains(*session_id))
            .cloned()
            .collect();

        if missing.is_empty() {
            info!(
                user_id = %user_id,
                sessions = ?session_ids,
                reason = reason.unwrap_or(""),
                "force session sync requested"
            );
        } else {
            warn!(
                user_id = %user_id,
                missing = ?missing,
                reason = reason.unwrap_or(""),
                "force session sync encountered unknown sessions"
            );
        }

        Ok(missing)
    }

    /// 创建会话（业务逻辑）
    /// 
    /// 如果 attributes 中包含 "session_id" 且会话不存在，则使用指定的 session_id
    /// 否则生成新的 UUID 作为 session_id
    /// 
    /// 如果会话已存在，则更新参与者，确保所有参与者都在会话中
    pub async fn create_session(
        &self,
        session_type: String,
        business_type: String,
        participants: Vec<SessionParticipant>,
        mut attributes: HashMap<String, String>,
        visibility: SessionVisibility,
    ) -> Result<Session> {
        // 尝试从 attributes 中提取指定的 session_id
        if let Some(requested_session_id) = attributes.remove("session_id") {
            // 验证会话ID格式（如果格式不正确，记录警告但继续处理，保持向后兼容）
            if let Err(e) = validate_session_id(&requested_session_id) {
                warn!(
                    session_id = %requested_session_id,
                    error = %e,
                    "Invalid session ID format, but continuing for backward compatibility"
                );
            }
            
            // 检查会话是否已存在
            if let Ok(Some(existing_session)) = self.session_repo.get_session(&requested_session_id).await {
                // 会话已存在，更新参与者（确保所有参与者都在会话中）
                debug!(
                    session_id = %requested_session_id,
                    participant_count = participants.len(),
                    "Session already exists, ensuring all participants are added"
                );
                
                // 获取需要添加的参与者（不在现有参与者列表中的）
                let existing_participant_ids: std::collections::HashSet<String> = existing_session
                    .participants
                    .iter()
                    .map(|p| p.user_id.clone())
                    .collect();
                
                let participants_to_add: Vec<SessionParticipant> = participants
                    .into_iter()
                    .filter(|p| !existing_participant_ids.contains(&p.user_id))
                    .collect();
                
                if !participants_to_add.is_empty() {
                    debug!(
                        session_id = %requested_session_id,
                        new_participants = participants_to_add.len(),
                        "Adding new participants to existing session"
                    );
                    self.session_repo
                        .manage_participants(
                            &requested_session_id,
                            &participants_to_add,
                            &[],
                            &[],
                        )
                        .await?;
                }
                
                // 返回现有会话
                Ok(existing_session)
            } else {
                // 会话不存在，使用指定的 session_id 创建新会话
                debug!(
                    session_id = %requested_session_id,
                    "Creating new session with provided session_id from attributes"
                );
                let session = Session {
                    session_id: requested_session_id.clone(),
                    session_type,
                    business_type,
                    display_name: None,
                    attributes,
                    participants,
                    visibility,
                    lifecycle_state: SessionLifecycleState::Active,
                    policy: None,
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                };

                self.session_repo.create_session(&session).await?;
                info!(session_id = %requested_session_id, "Session created with provided session_id");
                Ok(session)
            }
        } else {
            // 没有指定 session_id，根据会话类型生成
            let session_id = match session_type.as_str() {
                "group" => generate_server_session_id(SessionType::Group),
                "assistant" | "ai" => generate_server_session_id(SessionType::Ai),
                "system" => generate_server_session_id(SessionType::System),
                "customer" => generate_server_session_id(SessionType::Customer),
                "temp" => generate_server_session_id(SessionType::Temp),
                _ => {
                    // 默认使用UUID（向后兼容）
                    warn!(
                        session_type = %session_type,
                        "Unknown session type, using UUID for session_id (backward compatibility)"
                    );
                    Uuid::new_v4().to_string()
                }
            };
            
            let session = Session {
                session_id: session_id.clone(),
                session_type,
                business_type,
                display_name: None,
                attributes,
                participants,
                visibility,
                lifecycle_state: SessionLifecycleState::Active,
                policy: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };

            self.session_repo.create_session(&session).await?;
            info!(
                session_id = %session_id,
                "Session created with generated session_id"
            );
            Ok(session)
        }
    }

    /// 获取会话（业务逻辑）
    pub async fn get_session(&self, session_id: &str) -> Result<Option<Session>> {
        self.session_repo.get_session(session_id).await
    }

    /// 更新会话（业务逻辑）
    pub async fn update_session(
        &self,
        session_id: &str,
        display_name: Option<String>,
        attributes: Option<HashMap<String, String>>,
        visibility: Option<SessionVisibility>,
        lifecycle_state: Option<SessionLifecycleState>,
    ) -> Result<Session> {
        let mut session = self
            .session_repo
            .get_session(session_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", session_id))?;

        if let Some(name) = display_name {
            session.display_name = Some(name);
        }
        if let Some(attrs) = attributes {
            session.attributes = attrs;
        }
        if let Some(vis) = visibility {
            session.visibility = vis;
        }
        if let Some(state) = lifecycle_state {
            session.lifecycle_state = state;
        }
        session.updated_at = chrono::Utc::now();

        self.session_repo.update_session(&session).await?;
        info!(session_id = %session_id, "Session updated");
        Ok(session)
    }

    /// 删除会话（业务逻辑）
    pub async fn delete_session(&self, session_id: &str, hard_delete: bool) -> Result<()> {
        self.session_repo.delete_session(session_id, hard_delete).await?;
        info!(session_id = %session_id, hard_delete = hard_delete, "Session deleted");
        Ok(())
    }

    /// 管理参与者（业务逻辑）
    pub async fn manage_participants(
        &self,
        session_id: &str,
        to_add: Vec<SessionParticipant>,
        to_remove: Vec<String>,
        role_updates: Vec<(String, Vec<String>)>,
    ) -> Result<Vec<SessionParticipant>> {
        let participants = self
            .session_repo
            .manage_participants(session_id, &to_add, &to_remove, &role_updates)
            .await?;
        info!(
            session_id = %session_id,
            added = to_add.len(),
            removed = to_remove.len(),
            role_updates = role_updates.len(),
            "Participants managed"
        );
        Ok(participants)
    }

    /// 批量确认（业务逻辑）
    pub async fn batch_acknowledge(
        &self,
        user_id: &str,
        cursors: Vec<(String, i64)>,
    ) -> Result<()> {
        self.session_repo
            .batch_acknowledge(user_id, &cursors)
            .await?;
        info!(user_id = %user_id, count = cursors.len(), "Batch acknowledge completed");
        Ok(())
    }

    /// 标记消息为已读（业务逻辑）
    /// 
    /// 更新用户的 last_read_msg_seq，并重新计算未读数
    pub async fn mark_as_read(
        &self,
        user_id: &str,
        session_id: &str,
        seq: i64,
    ) -> Result<()> {
        self.session_repo
            .mark_as_read(user_id, session_id, seq)
            .await?;
        info!(
            user_id = %user_id,
            session_id = %session_id,
            seq,
            "Marked messages as read"
        );
        Ok(())
    }

    /// 获取未读数（业务逻辑）
    pub async fn get_unread_count(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<i32> {
        self.session_repo
            .get_unread_count(user_id, session_id)
            .await
    }

    /// 搜索会话（业务逻辑）
    pub async fn search_sessions(
        &self,
        user_id: Option<&str>,
        filters: Vec<SessionFilter>,
        sort: Vec<SessionSort>,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SessionSummary>, usize)> {
        self.session_repo
            .search_sessions(user_id, &filters, &sort, limit, offset)
            .await
    }
}

fn parse_cursor(cursor: Option<&str>) -> (Option<i64>, String) {
    if let Some(cursor) = cursor {
        if let Some((ts, id)) = cursor.split_once(':') {
            if let Ok(parsed) = ts.parse::<i64>() {
                return (Some(parsed), id.to_string());
            }
        }
    }
    (None, String::new())
}
