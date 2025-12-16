//! 会话管理领域服务
//!
//! 封装会话管理的核心业务逻辑，包括注册、注销、心跳等

use async_trait::async_trait;
use flare_proto::signaling::{HeartbeatRequest, LoginRequest, LogoutRequest};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result}; // 使用 flare_server_core 的 Result 类型
use std::sync::Arc;
use tracing::{info, instrument, warn};

use crate::domain::repository::SignalingGateway;
use crate::domain::service::ConnectionQualityService;

/// 会话管理领域服务
///
/// 职责：
/// - 封装会话注册/注销逻辑
/// - 封装心跳管理逻辑
/// - 提供会话生命周期管理
pub struct SessionDomainService {
    signaling_gateway: Arc<dyn SignalingGateway>,
    quality_service: Arc<ConnectionQualityService>,
    gateway_id: String,
}

impl SessionDomainService {
    pub fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        quality_service: Arc<ConnectionQualityService>,
        gateway_id: String,
    ) -> Self {
        Self {
            signaling_gateway,
            quality_service,
            gateway_id,
        }
    }

    /// 注册会话（在线状态）
    ///
    /// 将用户的连接信息注册到 Signaling Online 服务
    #[instrument(skip(self), fields(user_id, device_id, gateway_id = %self.gateway_id))]
    pub async fn register_session(
        &self,
        user_id: &str,
        device_id: &str,
        connection_id: Option<&str>,
    ) -> Result<String> {
        use uuid::Uuid;

        let session_id = Uuid::new_v4().to_string();
        let server_id = self.gateway_id.clone();

        // 构建 metadata，包含 gateway_id
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("gateway_id".to_string(), self.gateway_id.clone());

        let login_request = LoginRequest {
            user_id: user_id.to_string(),
            token: String::new(),
            device_id: device_id.to_string(),
            server_id: server_id.clone(),
            metadata,
            context: None,
            tenant: None,
            device_platform: "unknown".to_string(),
            app_version: "unknown".to_string(),
            desired_conflict_strategy: 0,
            device_priority: 2, // Normal 优先级
            token_version: 0,
            initial_quality: None,
            resume_session_id: String::new(),
        };

        // 调用 Signaling Online 服务，添加超时保护
        let login_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.signaling_gateway.login(login_request),
        )
        .await;

        match login_result {
            Ok(Ok(response)) => {
                if response.success {
                    info!(
                        user_id = %user_id,
                        gateway_id = %self.gateway_id,
                        session_id = %response.session_id,
                        "Session registered successfully"
                    );
                    Ok(response.session_id)
                } else {
                    warn!(
                        user_id = %user_id,
                        error = %response.error_message,
                        "Failed to register session"
                    );
                    Err(ErrorBuilder::new(
                        ErrorCode::OperationFailed,
                        format!("Failed to register session: {}", response.error_message),
                    )
                    .build_error())
                }
            }
            Ok(Err(e)) => {
                warn!(
                    ?e,
                    user_id = %user_id,
                    "Failed to call signaling login"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Signaling login failed: {}", e),
                )
                .build_error())
            }
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    "Timeout while calling signaling login (5s)"
                );
                Err(
                    ErrorBuilder::new(ErrorCode::OperationTimeout, "Signaling login timeout")
                        .build_error(),
                )
            }
        }
    }

    /// 注销会话（离线状态）
    ///
    /// 通知 Signaling Online 服务注销用户会话
    #[instrument(skip(self), fields(user_id, gateway_id = %self.gateway_id))]
    pub async fn unregister_session(&self, user_id: &str, session_id: Option<&str>) -> Result<()> {
        let logout_request = LogoutRequest {
            user_id: user_id.to_string(),
            session_id: session_id.unwrap_or("").to_string(),
            context: None,
            tenant: None,
        };

        if let Err(e) = self.signaling_gateway.logout(logout_request).await {
            warn!(
                ?e,
                user_id = %user_id,
                "Failed to call signaling logout"
            );
            return Err(ErrorBuilder::new(
                ErrorCode::InternalError,
                format!("Signaling logout failed: {}", e),
            )
            .build_error());
        }

        info!(
            user_id = %user_id,
            "Session unregistered successfully"
        );
        Ok(())
    }

    /// 刷新会话心跳
    ///
    /// 向 Signaling Online 服务发送心跳，保持会话活跃
    #[instrument(skip(self), fields(user_id, gateway_id = %self.gateway_id))]
    pub async fn refresh_heartbeat(
        &self,
        user_id: &str,
        session_id: &str,
        connection_id: Option<&str>,
    ) -> Result<()> {
        // 获取连接质量信息（如果提供了connection_id）
        let current_quality = if let Some(conn_id) = connection_id {
            if let Some(metrics) = self.quality_service.get_quality(conn_id).await {
                Some(flare_proto::common::ConnectionQuality {
                    rtt_ms: metrics.rtt_ms,
                    packet_loss_rate: metrics.packet_loss_rate,
                    last_measure_ts: chrono::Utc::now().timestamp_millis(),
                    network_type: metrics.network_type.clone(),
                    signal_strength: 0, // 在实际实现中应该从设备获取
                })
            } else {
                None
            }
        } else {
            None
        };

        let heartbeat_request = HeartbeatRequest {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            context: None,
            tenant: None,
            current_quality, // 使用从链接质量服务获取的质量信息
        };

        // 添加超时保护
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.signaling_gateway.heartbeat(heartbeat_request),
        )
        .await
        {
            Ok(Ok(_)) => {
                tracing::debug!(
                    user_id = %user_id,
                    session_id = %session_id,
                    "Heartbeat sent successfully"
                );
                Ok(())
            }
            Ok(Err(e)) => {
                warn!(
                    error = %e,
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to send heartbeat"
                );
                Err(
                    ErrorBuilder::new(ErrorCode::InternalError, format!("Heartbeat failed: {}", e))
                        .build_error(),
                )
            }
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    session_id = %session_id,
                    "Timeout sending heartbeat (3s)"
                );
                Err(
                    ErrorBuilder::new(ErrorCode::OperationTimeout, "Heartbeat timeout")
                        .build_error(),
                )
            }
        }
    }
}
