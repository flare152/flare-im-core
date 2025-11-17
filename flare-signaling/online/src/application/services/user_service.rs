//! 用户服务 - 实现 UserService 的所有功能

use std::sync::Arc;

use chrono::Utc;
use flare_proto::signaling::{
    BatchGetUserPresenceRequest, BatchGetUserPresenceResponse, DeviceInfo, GetDeviceRequest,
    GetDeviceResponse, GetUserPresenceRequest, GetUserPresenceResponse, KickDeviceRequest,
    KickDeviceResponse, ListUserDevicesRequest, ListUserDevicesResponse, UserPresence,
};
use flare_server_core::error::{ErrorCode, Result};
use prost_types::Timestamp;
use tracing::{info, warn};

use crate::domain::repositories::SessionRepository;
use crate::util;

pub struct UserService {
    session_repository: Arc<dyn SessionRepository>,
}

impl UserService {
    pub fn new(session_repository: Arc<dyn SessionRepository>) -> Self {
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
        let session = self
            .session_repository
            .get_session_by_device(user_id, device_id)
            .await?;

        if let Some(session) = session {
            // 删除会话
            self.session_repository
                .remove_session(&session.session_id, user_id)
                .await?;

            info!(
                user_id = %user_id,
                device_id = %device_id,
                session_id = %session.session_id,
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

