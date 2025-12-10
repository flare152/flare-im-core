//! 用户领域服务 - 包含所有业务逻辑实现

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use flare_proto::signaling::online::{
    BatchGetUserPresenceRequest, BatchGetUserPresenceResponse, DeviceInfo, GetDeviceRequest,
    GetDeviceResponse, GetUserPresenceRequest, GetUserPresenceResponse, KickDeviceRequest,
    KickDeviceResponse, ListUserDevicesRequest, ListUserDevicesResponse, UserPresence,
};
use flare_server_core::error::ErrorCode;
use prost_types::Timestamp;
use tracing::{info, warn};

use crate::domain::repository::SessionRepository;
use crate::util;

/// 用户领域服务 - 包含所有业务逻辑
pub struct UserService {
    session_repository: Arc<dyn SessionRepository + Send + Sync>,
}

impl UserService {
    pub fn new(session_repository: Arc<dyn SessionRepository + Send + Sync>) -> Self {
        Self {
            session_repository,
        }
    }

    /// 查询用户在线状态
    pub async fn get_user_presence(
        &self,
        request: GetUserPresenceRequest,
    ) -> Result<GetUserPresenceResponse> {
        let user_id = &request.user_id;

        // 获取用户的所有设备
        let devices = self.session_repository.list_user_devices(user_id).await?;
        
        // 计算在线状态
        let is_online = !devices.is_empty();
        let last_seen = devices
            .iter()
            .map(|d| d.last_active_time)
            .max()
            .unwrap_or_else(Utc::now);

        let presence = UserPresence {
            user_id: user_id.clone(),
            is_online,
            devices: devices
                .into_iter()
                .map(|d| DeviceInfo {
                    device_id: d.device_id,
                    platform: d.platform,
                    model: d.model.unwrap_or_default(),
                    os_version: d.os_version.unwrap_or_default(),
                    last_active_time: Some(Timestamp {
                        seconds: d.last_active_time.timestamp(),
                        nanos: d.last_active_time.timestamp_subsec_nanos() as i32,
                    }),
                    priority: 0,  // TODO: 从 SessionRecord 获取
                    token_version: 0,  // TODO: 从 SessionRecord 获取
                    connection_quality: None,  // TODO: 从 SessionRecord 获取
                    session_id: String::new(),  // TODO: 从 SessionRecord 获取
                    gateway_id: String::new(),  // TODO: 从 SessionRecord 获取
                    server_id: String::new(),  // TODO: 从 SessionRecord 获取
                })
                .collect(),
            last_seen: Some(Timestamp {
                seconds: last_seen.timestamp(),
                nanos: last_seen.timestamp_subsec_nanos() as i32,
            }),
        };

        Ok(GetUserPresenceResponse {
            presence: Some(presence),
            status: util::rpc_status_ok(),
        })
    }

    /// 批量查询在线状态
    pub async fn batch_get_user_presence(
        &self,
        request: BatchGetUserPresenceRequest,
    ) -> Result<BatchGetUserPresenceResponse> {
        let user_ids = &request.user_ids;

        if user_ids.is_empty() {
            return Ok(BatchGetUserPresenceResponse {
                presences: std::collections::HashMap::new(),
                status: util::rpc_status_ok(),
            });
        }

        if user_ids.len() > 100 {
            return Ok(BatchGetUserPresenceResponse {
                presences: std::collections::HashMap::new(),
                status: util::rpc_status_error(
                    ErrorCode::InvalidParameter,
                    "maximum 100 user_ids allowed",
                ),
            });
        }

        let mut presences = std::collections::HashMap::new();

        for user_id in user_ids {
            match self.get_user_presence(GetUserPresenceRequest {
                user_id: user_id.clone(),
                context: request.context.clone(),
                tenant: request.tenant.clone(),
            }).await {
                Ok(response) => {
                    if let Some(presence) = response.presence {
                        presences.insert(user_id.clone(), presence);
                    }
                }
                Err(err) => {
                    warn!(user_id = %user_id, error = %err, "failed to get user presence");
                }
            }
        }

        Ok(BatchGetUserPresenceResponse {
            presences,
            status: util::rpc_status_ok(),
        })
    }

    /// 列出用户设备
    pub async fn list_user_devices(
        &self,
        request: ListUserDevicesRequest,
    ) -> Result<ListUserDevicesResponse> {
        let user_id = &request.user_id;

        let devices = self.session_repository.list_user_devices(user_id).await?;

        Ok(ListUserDevicesResponse {
            devices: devices
                .into_iter()
                .map(|d| DeviceInfo {
                    device_id: d.device_id,
                    platform: d.platform,
                    model: d.model.unwrap_or_default(),
                    os_version: d.os_version.unwrap_or_default(),
                    last_active_time: Some(Timestamp {
                        seconds: d.last_active_time.timestamp(),
                        nanos: d.last_active_time.timestamp_subsec_nanos() as i32,
                    }),
                    priority: 0,  // TODO: 从 SessionRecord 获取
                    token_version: 0,  // TODO: 从 SessionRecord 获取
                    connection_quality: None,  // TODO: 从 SessionRecord 获取
                    session_id: String::new(),  // TODO: 从 SessionRecord 获取
                    gateway_id: String::new(),  // TODO: 从 SessionRecord 获取
                    server_id: String::new(),  // TODO: 从 SessionRecord 获取
                })
                .collect(),
            status: util::rpc_status_ok(),
        })
    }

    /// 踢出设备
    pub async fn kick_device(&self, request: KickDeviceRequest) -> Result<KickDeviceResponse> {
        let user_id = &request.user_id;
        let device_id = &request.device_id;

        // 查找设备对应的会话
        let user_vo = crate::domain::value_object::UserId::new(user_id.to_string()).map_err(|e| anyhow::anyhow!(e))?;
        let device_vo = crate::domain::value_object::DeviceId::new(device_id.to_string()).map_err(|e| anyhow::anyhow!(e))?;
        let session = self
            .session_repository
            .get_session_by_device(&user_vo, &device_vo)
            .await?;

        if let Some(session) = session {
            // 删除会话
            self.session_repository
                .remove_session(&session.id(), &user_vo)
                .await?;

            info!(
                user_id = %user_id,
                device_id = %device_id,
                session_id = %session.id().as_str(),
                "device kicked"
            );

            Ok(KickDeviceResponse {
                success: true,
                status: util::rpc_status_ok(),
            })
        } else {
            Ok(KickDeviceResponse {
                success: false,
                status: util::rpc_status_error(
                    ErrorCode::UserNotFound,
                    "device not found",
                ),
            })
        }
    }

    /// 查询设备信息
    pub async fn get_device(&self, request: GetDeviceRequest) -> Result<GetDeviceResponse> {
        let user_id = &request.user_id;
        let device_id = &request.device_id;

        let device = self.session_repository.get_device(user_id, device_id).await?;

        if let Some(device) = device {
            Ok(GetDeviceResponse {
                device: Some(DeviceInfo {
                    device_id: device.device_id,
                    platform: device.platform,
                    model: device.model.unwrap_or_default(),
                    os_version: device.os_version.unwrap_or_default(),
                    last_active_time: Some(Timestamp {
                        seconds: device.last_active_time.timestamp(),
                        nanos: device.last_active_time.timestamp_subsec_nanos() as i32,
                    }),
                    priority: 0,  // TODO: 从 SessionRecord 获取
                    token_version: 0,  // TODO: 从 SessionRecord 获取
                    connection_quality: None,  // TODO: 从 SessionRecord 获取
                    session_id: String::new(),  // TODO: 从 SessionRecord 获取
                    gateway_id: String::new(),  // TODO: 从 SessionRecord 获取
                    server_id: String::new(),  // TODO: 从 SessionRecord 获取
                }),
                status: util::rpc_status_ok(),
            })
        } else {
            Ok(GetDeviceResponse {
                device: None,
                status: util::rpc_status_error(ErrorCode::InvalidParameter, "device not found"),
            })
        }
    }
}

