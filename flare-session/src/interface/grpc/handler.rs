use std::sync::Arc;

use chrono::{DateTime, Utc};
use flare_proto::session::session_service_server::SessionService;
use flare_proto::session::{
    BatchAcknowledgeRequest, BatchAcknowledgeResponse, CreateSessionRequest,
    CreateSessionResponse, DeleteSessionRequest, DeleteSessionResponse,
    DevicePresence as ProtoDevicePresence, DeviceState as ProtoDeviceState,
    ForceSessionSyncRequest, ForceSessionSyncResponse, ListSessionsRequest,
    ListSessionsResponse, ManageParticipantsRequest, ManageParticipantsResponse,
    SearchSessionsRequest, SearchSessionsResponse, SessionBootstrapRequest,
    SessionBootstrapResponse, SessionPolicy as ProtoSessionPolicy,
    SessionSummary as ProtoSessionSummary, SyncMessagesRequest, SyncMessagesResponse,
    UpdateCursorRequest, UpdateCursorResponse, UpdatePresenceRequest, UpdatePresenceResponse,
    UpdateSessionRequest, UpdateSessionResponse,
};
use flare_proto::common;
use flare_server_core::error;
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::application::handlers::{SessionCommandHandler, SessionQueryHandler};
use crate::application::commands::{
    BatchAcknowledgeCommand, CreateSessionCommand, DeleteSessionCommand,
    ForceSessionSyncCommand, ManageParticipantsCommand, UpdateCursorCommand,
    UpdatePresenceCommand, UpdateSessionCommand,
};
use crate::application::queries::{
    ListSessionsQuery, SearchSessionsQuery, SessionBootstrapQuery,
    SyncMessagesQuery,
};
use crate::domain::model::{
    ConflictResolutionPolicy, DevicePresence, DeviceState, Session, SessionFilter,
    SessionLifecycleState, SessionParticipant, SessionPolicy, SessionSort, SessionSummary,
    SessionVisibility,
};

#[derive(Clone)]
pub struct SessionGrpcHandler {
    command_handler: Arc<SessionCommandHandler>,
    query_handler: Arc<SessionQueryHandler>,
}

impl SessionGrpcHandler {
    pub fn new(
        command_handler: Arc<SessionCommandHandler>,
        query_handler: Arc<SessionQueryHandler>,
    ) -> Self {
        Self {
            command_handler,
            query_handler,
        }
    }
}

#[tonic::async_trait]
impl SessionService for SessionGrpcHandler {
    async fn session_bootstrap(
        &self,
        request: Request<SessionBootstrapRequest>,
    ) -> Result<Response<SessionBootstrapResponse>, Status> {
        let req = request.into_inner();
        let cursor_map = req.client_cursor_map;

        let include_recent = req.include_recent_messages;
        let recent_limit = if req.recent_message_limit > 0 {
            Some(req.recent_message_limit)
        } else {
            None
        };

        let bootstrap = self
            .query_handler
            .handle_session_bootstrap(SessionBootstrapQuery {
                user_id: req.user_id.clone(),
                client_cursor: cursor_map.clone(),
                include_recent,
                recent_limit,
            })
            .await
            .map_err(internal_error)?;

        let response = SessionBootstrapResponse {
            sessions: bootstrap.summaries.into_iter().map(proto_summary).collect(),
            recent_messages: bootstrap.recent_messages,
            devices: bootstrap.devices.into_iter().map(proto_device).collect(),
            server_cursor_map: bootstrap.cursor_map,
            policy: Some(proto_policy(bootstrap.policy)),
            status: Some(error::ok_status()),
        };

        Ok(Response::new(response))
    }

    async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        let req = request.into_inner();
        let (summaries, next_cursor, has_more) = self
            .query_handler
            .handle_list_sessions(ListSessionsQuery {
                user_id: req.user_id.clone(),
                cursor: if req.cursor.is_empty() {
                    None
                } else {
                    Some(req.cursor)
                },
                limit: if req.limit > 0 { req.limit } else { 20 },
            })
            .await
            .map_err(internal_error)?;

        let response = ListSessionsResponse {
            sessions: summaries.into_iter().map(proto_summary).collect(),
            next_cursor: next_cursor.unwrap_or_default(),
            has_more,
            status: Some(error::ok_status()),
        };

        Ok(Response::new(response))
    }

    async fn sync_messages(
        &self,
        request: Request<SyncMessagesRequest>,
    ) -> Result<Response<SyncMessagesResponse>, Status> {
        let req = request.into_inner();

        let result = self
            .query_handler
            .handle_sync_messages(SyncMessagesQuery {
                session_id: req.session_id.clone(),
                since_ts: req.since_ts,
                cursor: if req.cursor.is_empty() {
                    None
                } else {
                    Some(req.cursor)
                },
                limit: if req.limit > 0 { req.limit } else { 50 },
            })
            .await
            .map_err(|err| {
                if err.to_string().contains("message provider not configured") {
                    failed_precondition(err)
                } else {
                    internal_error(err)
                }
            })?;

        let response = SyncMessagesResponse {
            messages: result.messages,
            next_cursor: result.next_cursor.unwrap_or_default(),
            server_cursor_ts: result.server_cursor_ts.unwrap_or_default(),
            status: Some(error::ok_status()),
        };

        Ok(Response::new(response))
    }

    async fn update_cursor(
        &self,
        request: Request<UpdateCursorRequest>,
    ) -> Result<Response<UpdateCursorResponse>, Status> {
        let req = request.into_inner();
        self.command_handler
            .handle_update_cursor(UpdateCursorCommand {
                user_id: req.user_id.clone(),
                session_id: req.session_id.clone(),
                message_ts: req.message_ts,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(UpdateCursorResponse {
            status: Some(error::ok_status()),
        }))
    }

    async fn update_presence(
        &self,
        request: Request<UpdatePresenceRequest>,
    ) -> Result<Response<UpdatePresenceResponse>, Status> {
        let req = request.into_inner();
        let state = match ProtoDeviceState::try_from(req.state).ok() {
            Some(ProtoDeviceState::Unspecified) | None => DeviceState::Unspecified,
            Some(ProtoDeviceState::Online) => DeviceState::Online,
            Some(ProtoDeviceState::Offline) => DeviceState::Offline,
            Some(ProtoDeviceState::Conflict) => DeviceState::Conflict,
        };

        let resolution = ConflictResolutionPolicy::from_proto(req.resolution);
        let resolution = if resolution == ConflictResolutionPolicy::Unspecified {
            None
        } else {
            Some(resolution)
        };

        self.command_handler
            .handle_update_presence(UpdatePresenceCommand {
                user_id: req.user_id.clone(),
                device_id: req.device_id.clone(),
                device_platform: if req.device_platform.is_empty() {
                    None
                } else {
                    Some(req.device_platform)
                },
                state,
                conflict_resolution: resolution,
                notify_conflict: req.notify_conflict,
                conflict_reason: if req.conflict_reason.is_empty() {
                    None
                } else {
                    Some(req.conflict_reason)
                },
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(UpdatePresenceResponse {
            status: Some(error::ok_status()),
        }))
    }

    async fn force_session_sync(
        &self,
        request: Request<ForceSessionSyncRequest>,
    ) -> Result<Response<ForceSessionSyncResponse>, Status> {
        let req = request.into_inner();
        let missing = self
            .command_handler
            .handle_force_session_sync(ForceSessionSyncCommand {
                user_id: req.user_id.clone(),
                session_ids: req.session_ids.clone(),
                reason: if req.reason.is_empty() {
                    None
                } else {
                    Some(req.reason)
                },
            })
            .await
            .map_err(internal_error)?;

        if !missing.is_empty() {
            return Err(Status::failed_precondition(format!(
                "unknown sessions: {}",
                missing.join(",")
            )));
        }

        Ok(Response::new(ForceSessionSyncResponse {
            status: Some(error::ok_status()),
        }))
    }

    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        let req = request.into_inner();

        let participants: Vec<SessionParticipant> = req
            .participants
            .into_iter()
            .map(|p| SessionParticipant {
                user_id: p.user_id,
                roles: p.roles,
                muted: p.muted,
                pinned: p.pinned,
                attributes: p.attributes,
            })
            .collect();

        let visibility = SessionVisibility::from_proto(req.visibility);

        let session = self
            .command_handler
            .handle_create_session(CreateSessionCommand {
                session_type: req.session_type,
                business_type: req.business_type,
                participants,
                attributes: req.attributes,
                visibility,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(CreateSessionResponse {
            session: Some(domain_to_proto_session(session)),
            status: Some(error::ok_status()),
        }))
    }

    async fn update_session(
        &self,
        request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        let req = request.into_inner();

        let display_name = if req.display_name.is_empty() {
            None
        } else {
            Some(req.display_name)
        };

        let visibility = if req.visibility == 0 {
            None
        } else {
            Some(SessionVisibility::from_proto(req.visibility))
        };

        let lifecycle_state = if req.lifecycle_state == 0 {
            None
        } else {
            Some(SessionLifecycleState::from_proto(req.lifecycle_state))
        };

        let session = self
            .command_handler
            .handle_update_session(UpdateSessionCommand {
                session_id: req.session_id.clone(),
                display_name,
                attributes: if req.attributes.is_empty() {
                    None
                } else {
                    Some(req.attributes)
                },
                visibility,
                lifecycle_state,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(UpdateSessionResponse {
            session: Some(domain_to_proto_session(session)),
            status: Some(error::ok_status()),
        }))
    }

    async fn delete_session(
        &self,
        request: Request<DeleteSessionRequest>,
    ) -> Result<Response<DeleteSessionResponse>, Status> {
        let req = request.into_inner();

        self.command_handler
            .handle_delete_session(DeleteSessionCommand {
                session_id: req.session_id.clone(),
                hard_delete: req.hard_delete,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(DeleteSessionResponse {
            status: Some(error::ok_status()),
        }))
    }

    async fn manage_participants(
        &self,
        request: Request<ManageParticipantsRequest>,
    ) -> Result<Response<ManageParticipantsResponse>, Status> {
        let req = request.into_inner();

        let to_add: Vec<SessionParticipant> = req
            .to_add
            .into_iter()
            .map(|p| SessionParticipant {
                user_id: p.user_id,
                roles: p.roles,
                muted: p.muted,
                pinned: p.pinned,
                attributes: p.attributes,
            })
            .collect();

        let role_updates: Vec<(String, Vec<String>)> = req
            .role_updates
            .into_iter()
            .map(|u| (u.user_id, u.roles))
            .collect();

        let participants = self
            .command_handler
            .handle_manage_participants(ManageParticipantsCommand {
                session_id: req.session_id.clone(),
                to_add,
                to_remove: req.to_remove,
                role_updates,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(ManageParticipantsResponse {
            participants: participants
                .into_iter()
                .map(|p| flare_proto::session::SessionParticipant {
                    user_id: p.user_id,
                    roles: p.roles,
                    muted: p.muted,
                    pinned: p.pinned,
                    attributes: p.attributes,
                })
                .collect(),
            status: Some(error::ok_status()),
        }))
    }

    async fn batch_acknowledge(
        &self,
        request: Request<BatchAcknowledgeRequest>,
    ) -> Result<Response<BatchAcknowledgeResponse>, Status> {
        let req = request.into_inner();

        let cursors: Vec<(String, i64)> = req
            .cursors
            .into_iter()
            .map(|c| (c.session_id, c.message_ts))
            .collect();

        self.command_handler
            .handle_batch_acknowledge(BatchAcknowledgeCommand {
                user_id: req.user_id.clone(),
                cursors,
            })
            .await
            .map_err(internal_error)?;

        Ok(Response::new(BatchAcknowledgeResponse {
            status: Some(error::ok_status()),
        }))
    }

    async fn search_sessions(
        &self,
        request: Request<SearchSessionsRequest>,
    ) -> Result<Response<SearchSessionsResponse>, Status> {
        let req = request.into_inner();

        // 从protobuf FilterExpression转换为domain models
        let mut filters = Vec::new();
        for filter_expr in &req.filters {
            let filter = match filter_expr.field.as_str() {
                "session_type" => {
                    if !filter_expr.values.is_empty() {
                        Some(SessionFilter {
                            session_type: Some(filter_expr.values[0].clone()),
                            business_type: None,
                            lifecycle_state: None,
                            visibility: None,
                            participant_user_id: None,
                        })
                    } else {
                        None
                    }
                }
                "business_type" => {
                    if !filter_expr.values.is_empty() {
                        Some(SessionFilter {
                            session_type: None,
                            business_type: Some(filter_expr.values[0].clone()),
                            lifecycle_state: None,
                            visibility: None,
                            participant_user_id: None,
                        })
                    } else {
                        None
                    }
                }
                "lifecycle_state" => {
                    if !filter_expr.values.is_empty() {
                        let state_str = &filter_expr.values[0];
                        let state = match state_str.as_str() {
                            "active" => SessionLifecycleState::Active,
                            "suspended" => SessionLifecycleState::Suspended,
                            "archived" => SessionLifecycleState::Archived,
                            "deleted" => SessionLifecycleState::Deleted,
                            _ => SessionLifecycleState::Unspecified,
                        };
                        Some(SessionFilter {
                            session_type: None,
                            business_type: None,
                            lifecycle_state: Some(state),
                            visibility: None,
                            participant_user_id: None,
                        })
                    } else {
                        None
                    }
                }
                "visibility" => {
                    if !filter_expr.values.is_empty() {
                        let vis_str = &filter_expr.values[0];
                        let vis = match vis_str.as_str() {
                            "private" => SessionVisibility::Private,
                            "tenant" => SessionVisibility::Tenant,
                            "public" => SessionVisibility::Public,
                            _ => SessionVisibility::Unspecified,
                        };
                        Some(SessionFilter {
                            session_type: None,
                            business_type: None,
                            lifecycle_state: None,
                            visibility: Some(vis),
                            participant_user_id: None,
                        })
                    } else {
                        None
                    }
                }
                "participant_user_id" => {
                    if !filter_expr.values.is_empty() {
                        Some(SessionFilter {
                            session_type: None,
                            business_type: None,
                            lifecycle_state: None,
                            visibility: None,
                            participant_user_id: Some(filter_expr.values[0].clone()),
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(f) = filter {
                filters.push(f);
            }
        }

        // 从protobuf SortExpression转换为domain models
        let sort: Vec<SessionSort> = req
            .sort
            .into_iter()
            .map(|s| SessionSort {
                field: s.field,
                ascending: s.direction == common::SortDirection::Asc as i32,
            })
            .collect();

        let limit = req
            .pagination
            .as_ref()
            .map(|p| p.limit as usize)
            .unwrap_or(20)
            .min(1000);
        let offset = 0; // 分页使用cursor，offset暂时为0

        let (summaries, total) = self
            .query_handler
            .handle_search_sessions(SearchSessionsQuery {
                user_id: None,
                filters,
                sort,
                limit,
                offset,
            })
            .await
            .map_err(internal_error)?;

        // 更新pagination信息
        let mut pagination = req.pagination.unwrap_or_default();
        pagination.total_size = total as i64;
        pagination.has_more = summaries.len() >= limit;

        Ok(Response::new(SearchSessionsResponse {
            sessions: summaries.into_iter().map(proto_summary).collect(),
            pagination: Some(pagination),
            status: Some(error::ok_status()),
        }))
    }
}

fn proto_summary(summary: SessionSummary) -> ProtoSessionSummary {
    let last_message_time = summary.last_message_time.and_then(timestamp_from_datetime);

    ProtoSessionSummary {
        session_id: summary.session_id,
        session_type: summary.session_type.unwrap_or_default(),
        business_type: summary.business_type.unwrap_or_default(),
        last_message_id: summary.last_message_id.unwrap_or_default(),
        last_message_time,
        last_sender_id: summary.last_sender_id.unwrap_or_default(),
        last_message_type: summary.last_message_type.unwrap_or_default(),
        last_content_type: summary.last_content_type.unwrap_or_default(),
        unread_count: summary.unread_count,
        metadata: summary.metadata,
        server_cursor_ts: summary.server_cursor_ts.unwrap_or_default(),
        display_name: summary.display_name.unwrap_or_default(),
    }
}

fn proto_device(device: DevicePresence) -> ProtoDevicePresence {
    let last_seen_at = device.last_seen_at.and_then(timestamp_from_datetime);

    ProtoDevicePresence {
        device_id: device.device_id,
        device_platform: device.device_platform.unwrap_or_default(),
        state: device.state.as_proto(),
        last_seen_at,
    }
}

fn proto_policy(policy: SessionPolicy) -> ProtoSessionPolicy {
    ProtoSessionPolicy {
        conflict_resolution: policy.conflict_resolution.as_proto(),
        max_devices: policy.max_devices,
        allow_anonymous: policy.allow_anonymous,
        allow_history_sync: policy.allow_history_sync,
        metadata: policy.metadata,
    }
}

fn timestamp_from_datetime(dt: DateTime<Utc>) -> Option<Timestamp> {
    Some(Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    })
}

fn internal_error(err: anyhow::Error) -> Status {
    Status::internal(err.to_string())
}

fn failed_precondition(err: anyhow::Error) -> Status {
    Status::failed_precondition(err.to_string())
}

fn domain_to_proto_session(session: Session) -> flare_proto::session::Session {
    flare_proto::session::Session {
        session_id: session.session_id,
        session_type: session.session_type,
        business_type: session.business_type,
        attributes: session.attributes,
        participants: session
            .participants
            .into_iter()
            .map(|p| flare_proto::session::SessionParticipant {
                user_id: p.user_id,
                roles: p.roles,
                muted: p.muted,
                pinned: p.pinned,
                attributes: p.attributes,
            })
            .collect(),
        visibility: session.visibility.as_proto(),
        lifecycle_state: session.lifecycle_state.as_proto(),
        created_at: timestamp_from_datetime(session.created_at),
        updated_at: timestamp_from_datetime(session.updated_at),
        policy: session.policy.map(proto_policy),
    }
}
