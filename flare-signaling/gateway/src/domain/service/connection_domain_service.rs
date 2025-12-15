//! 连接管理领域服务
//!
//! 封装连接管理的核心业务逻辑

use std::sync::Arc;
use flare_server_core::error::{Result, ErrorBuilder, ErrorCode};
use flare_proto::signaling::{LoginRequest, LogoutRequest, HeartbeatRequest};
use tracing::{info, warn, instrument};

use crate::domain::repository::SignalingGateway;
use crate::domain::service::ConnectionQualityService;

/// 连接管理领域服务配置
#[derive(Debug, Clone)]
pub struct ConnectionDomainServiceConfig {
    /// 网关ID
    pub gateway_id: String,
}

impl Default for ConnectionDomainServiceConfig {
    fn default() -> Self {
        Self {
            gateway_id: "gateway-1".to_string(),
        }
    }
}

/// 连接管理领域服务
///
/// 职责：
/// - 封装连接注册/注销逻辑
/// - 封装心跳管理逻辑
/// - 提供连接生命周期管理
pub struct ConnectionDomainService {
    signaling_gateway: Arc<dyn SignalingGateway>,
    quality_service: Arc<ConnectionQualityService>,
    config: ConnectionDomainServiceConfig,
}

impl ConnectionDomainService {
    pub fn new(
        signaling_gateway: Arc<dyn SignalingGateway>,
        quality_service: Arc<ConnectionQualityService>,
        config: ConnectionDomainServiceConfig,
    ) -> Self {
        Self {
            signaling_gateway,
            quality_service,
            config,
        }
    }

    /// 注册连接（在线状态）
    ///
    /// 将用户的连接信息注册到 Signaling Online 服务
    #[instrument(skip(self), fields(user_id, device_id))]
    pub async fn register_connection(
        &self,
        user_id: &str,
        device_id: &str,
        connection_id: Option<&str>,
    ) -> Result<String> {
        use uuid::Uuid;

        let session_id = Uuid::new_v4().to_string();
        let server_id = self.config.gateway_id.clone();

        // 构建 metadata，包含 gateway_id
        let mut metadata = std::collections::HashMap::new();
        metadata.insert("gateway_id".to_string(), self.config.gateway_id.clone());
        
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
            device_priority: 2,  // Normal 优先级
            token_version: 0,
            initial_quality: None,
            resume_session_id: String::new(),
        };

        // 调用 Signaling Online 服务，添加超时保护
        let login_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.signaling_gateway.login(login_request)
        ).await;

        match login_result {
            Ok(Ok(response)) => {
                if response.success {
                    info!(
                        user_id = %user_id,
                        session_id = %response.session_id,
                        "Connection registered successfully"
                    );
                    Ok(response.session_id)
                } else {
                    warn!(
                        user_id = %user_id,
                        error = %response.error_message,
                        "Failed to register connection"
                    );
                    Err(ErrorBuilder::new(
                        ErrorCode::OperationFailed,
                        format!("Failed to register connection: {}", response.error_message),
                    ).build_error())
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
                ).build_error())
            }
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    "Timeout while calling signaling login (5s)"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::OperationTimeout,
                    "Signaling login timeout",
                ).build_error())
            }
        }
    }

    /// 注销连接（离线状态）
    ///
    /// 通知 Signaling Online 服务注销用户连接
    #[instrument(skip(self), fields(user_id))]
    pub async fn unregister_connection(&self, user_id: &str, session_id: Option<&str>) -> Result<()> {
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
            ).build_error());
        }

        info!(
            user_id = %user_id,
            "Connection unregistered successfully"
        );
        Ok(())
    }

    /// 刷新连接心跳
    ///
    /// 向 Signaling Online 服务发送心跳，保持连接活跃
    #[instrument(skip(self), fields(user_id))]
    pub async fn refresh_heartbeat(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> Result<()> {
        // 从链接质量服务获取当前连接质量
        let current_quality = self.quality_service.get_quality(session_id).await
            .map(|metrics| flare_proto::common::ConnectionQuality {
                rtt_ms: metrics.rtt_ms,
                packet_loss_rate: metrics.packet_loss_rate,
                last_measure_ts: 0, // TODO: 填充正确的时间戳
                network_type: metrics.network_type,
                signal_strength: 0, // TODO: 填充正确的信号强度
            });

        let heartbeat_request = HeartbeatRequest {
            user_id: user_id.to_string(),
            session_id: session_id.to_string(),
            context: None,
            tenant: None,
            current_quality,
        };
        
        // 添加超时保护
        match tokio::time::timeout(
            std::time::Duration::from_secs(3),
            self.signaling_gateway.heartbeat(heartbeat_request)
        ).await {
            Ok(Ok(_)) => {
                tracing::debug!(
                    user_id = %user_id,
                    session_id = %session_id,
                    "Heartbeat sent successfully"
                );
                Ok(())
            },
            Ok(Err(e)) => {
                warn!(
                    error = %e,
                    user_id = %user_id,
                    session_id = %session_id,
                    "Failed to send heartbeat"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Heartbeat failed: {}", e),
                ).build_error())
            },
            Err(_) => {
                warn!(
                    user_id = %user_id,
                    session_id = %session_id,
                    "Timeout sending heartbeat (3s)"
                );
                Err(ErrorBuilder::new(
                    ErrorCode::OperationTimeout,
                    "Heartbeat timeout",
                ).build_error())
            }
        }
    }
}