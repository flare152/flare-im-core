use std::sync::Arc;

use anyhow::Result;
use tracing::{debug, info};

use crate::application::commands::{
    BatchAcknowledgeCommand, CreateSessionCommand, DeleteSessionCommand, ForceSessionSyncCommand,
    ManageParticipantsCommand, UpdateCursorCommand, UpdatePresenceCommand, UpdateSessionCommand,
};
use crate::application::queries::{
    ListSessionsQuery, SearchSessionsQuery, SessionBootstrapQuery, SyncMessagesQuery,
};
use crate::domain::service::session_domain_service::{
    SessionBootstrapOutput, SessionDomainService,
};

/// 会话命令处理器
pub struct SessionCommandHandler {
    domain_service: Arc<SessionDomainService>,
}

impl SessionCommandHandler {
    pub fn new(domain_service: Arc<SessionDomainService>) -> Self {
        Self { domain_service }
    }

    /// 处理批量确认命令
    pub async fn handle_batch_acknowledge(&self, command: BatchAcknowledgeCommand) -> Result<()> {
        debug!(
            user_id = %command.user_id,
            count = command.cursors.len(),
            "Handling batch acknowledge command"
        );

        self.domain_service
            .batch_acknowledge(&command.user_id, command.cursors)
            .await?;

        info!(user_id = %command.user_id, "Batch acknowledge completed");
        Ok(())
    }

    /// 处理创建会话命令
    pub async fn handle_create_session(
        &self,
        command: CreateSessionCommand,
    ) -> Result<crate::domain::model::Session> {
        debug!(
            session_type = %command.session_type,
            business_type = %command.business_type,
            "Handling create session command"
        );

        let session = self
            .domain_service
            .create_session(
                command.session_type,
                command.business_type,
                command.participants,
                command.attributes,
                command.visibility,
            )
            .await?;

        info!(session_id = %session.session_id, "Session created");
        Ok(session)
    }

    /// 处理删除会话命令
    pub async fn handle_delete_session(&self, command: DeleteSessionCommand) -> Result<()> {
        debug!(
            session_id = %command.session_id,
            hard_delete = command.hard_delete,
            "Handling delete session command"
        );

        self.domain_service
            .delete_session(&command.session_id, command.hard_delete)
            .await?;

        info!(session_id = %command.session_id, "Session deleted");
        Ok(())
    }

    /// 处理强制会话同步命令
    pub async fn handle_force_session_sync(
        &self,
        command: ForceSessionSyncCommand,
    ) -> Result<Vec<String>> {
        debug!(
            user_id = %command.user_id,
            session_count = command.session_ids.len(),
            "Handling force session sync command"
        );

        let missing = self
            .domain_service
            .force_session_sync(
                &command.user_id,
                &command.session_ids,
                command.reason.as_deref(),
            )
            .await?;

        Ok(missing)
    }

    /// 处理管理参与者命令
    pub async fn handle_manage_participants(
        &self,
        command: ManageParticipantsCommand,
    ) -> Result<Vec<crate::domain::model::SessionParticipant>> {
        debug!(
            session_id = %command.session_id,
            to_add = command.to_add.len(),
            to_remove = command.to_remove.len(),
            "Handling manage participants command"
        );

        let participants = self
            .domain_service
            .manage_participants(
                &command.session_id,
                command.to_add,
                command.to_remove,
                command.role_updates,
            )
            .await?;

        info!(session_id = %command.session_id, "Participants managed");
        Ok(participants)
    }

    /// 处理更新游标命令
    pub async fn handle_update_cursor(&self, command: UpdateCursorCommand) -> Result<()> {
        debug!(
            user_id = %command.user_id,
            session_id = %command.session_id,
            message_ts = command.message_ts,
            "Handling update cursor command"
        );

        self.domain_service
            .update_cursor(&command.user_id, &command.session_id, command.message_ts)
            .await?;

        Ok(())
    }

    /// 处理更新设备状态命令
    pub async fn handle_update_presence(&self, command: UpdatePresenceCommand) -> Result<()> {
        debug!(
            user_id = %command.user_id,
            device_id = %command.device_id,
            state = ?command.state,
            "Handling update presence command"
        );

        self.domain_service
            .update_presence(
                &command.user_id,
                &command.device_id,
                command.device_platform,
                command.state,
                command.conflict_resolution,
                command.notify_conflict,
                command.conflict_reason,
            )
            .await?;

        Ok(())
    }

    /// 处理更新会话命令
    pub async fn handle_update_session(
        &self,
        command: UpdateSessionCommand,
    ) -> Result<crate::domain::model::Session> {
        debug!(
            session_id = %command.session_id,
            "Handling update session command"
        );

        let session = self
            .domain_service
            .update_session(
                &command.session_id,
                command.display_name,
                command.attributes,
                command.visibility,
                command.lifecycle_state,
            )
            .await?;

        info!(session_id = %command.session_id, "Session updated");
        Ok(session)
    }
}

/// 会话查询处理器
pub struct SessionQueryHandler {
    domain_service: Arc<SessionDomainService>,
}

impl SessionQueryHandler {
    pub fn new(
        _session_repo: Arc<dyn crate::domain::repository::SessionRepository>,
        _message_provider: Option<
            Arc<dyn crate::domain::repository::MessageProvider + Send + Sync>,
        >,
        domain_service: Arc<SessionDomainService>,
    ) -> Self {
        Self { domain_service }
    }

    /// 处理列出会话查询
    pub async fn handle_list_sessions(
        &self,
        query: ListSessionsQuery,
    ) -> Result<(
        Vec<crate::domain::model::SessionSummary>,
        Option<String>,
        bool,
    )> {
        debug!(
            user_id = %query.user_id,
            cursor = ?query.cursor,
            limit = query.limit,
            "Handling list sessions query"
        );

        let result = self
            .domain_service
            .list_sessions(&query.user_id, query.cursor.as_deref(), query.limit)
            .await?;

        Ok(result)
    }

    /// 处理搜索会话查询
    pub async fn handle_search_sessions(
        &self,
        query: SearchSessionsQuery,
    ) -> Result<(Vec<crate::domain::model::SessionSummary>, usize)> {
        debug!(
            user_id = ?query.user_id,
            filter_count = query.filters.len(),
            sort_count = query.sort.len(),
            limit = query.limit,
            "Handling search sessions query"
        );

        let result = self
            .domain_service
            .search_sessions(
                query.user_id.as_deref(),
                query.filters,
                query.sort,
                query.limit,
                query.offset,
            )
            .await?;

        Ok(result)
    }

    /// 处理会话引导查询
    pub async fn handle_session_bootstrap(
        &self,
        query: SessionBootstrapQuery,
    ) -> Result<SessionBootstrapOutput> {
        debug!(
            user_id = %query.user_id,
            include_recent = query.include_recent,
            "Handling session bootstrap query"
        );

        let result = self
            .domain_service
            .bootstrap_session(
                &query.user_id,
                query.client_cursor,
                query.include_recent,
                query.recent_limit,
            )
            .await?;

        Ok(result)
    }

    /// 处理同步消息查询
    pub async fn handle_sync_messages(
        &self,
        query: SyncMessagesQuery,
    ) -> Result<crate::domain::model::MessageSyncResult> {
        debug!(
            session_id = %query.session_id,
            since_ts = query.since_ts,
            cursor = ?query.cursor,
            limit = query.limit,
            "Handling sync messages query"
        );

        let result = self
            .domain_service
            .sync_messages(
                &query.session_id,
                query.since_ts,
                query.cursor.as_deref(),
                query.limit,
            )
            .await?;

        Ok(result)
    }
}
