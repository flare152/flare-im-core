//! ACK上报发布器
//!
//! 使用 transport.proto 定义的 SendEnvelopeAck 作为核心 ACK 结构
//! Gateway 作为接入层，通过轻量的 gRPC 上报，不直接依赖 Kafka

use async_trait::async_trait;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, warn, debug};

/// ACK 状态值
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AckStatusValue {
    Success,
    Failed,
}

/// ACK 核心数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AckData {
    pub message_id: String,
    pub status: AckStatusValue,
    pub error_code: Option<i32>,
    pub error_message: Option<String>,
}

/// ACK 审计事件（用于上报）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AckAuditEvent {
    pub ack: AckData,
    pub user_id: String,
    pub connection_id: String,
    pub gateway_id: String,
    pub timestamp: i64,
    pub window_id: Option<String>,
    pub ack_seq: Option<i64>,
}

impl AckAuditEvent {
    pub fn ack_type(&self) -> &'static str {
        if self.window_id.is_some() {
            "window_ack"
        } else {
            "message_ack"
        }
    }
}

/// ACK 发布器 trait
#[async_trait]
pub trait AckPublisher: Send + Sync {
    async fn publish_ack(&self, event: &AckAuditEvent) -> Result<()>;
}



/// gRPC ACK 发布器（轻量，推荐）
/// 
/// Gateway 作为接入层，通过 gRPC 直接上报到 Push/Session 服务
/// 优势：
/// - ✅ 无需 Kafka 依赖，部署更轻量
/// - ✅ 直接通信，延迟更低（~2ms vs ~10ms）
/// - ✅ 失败降级，不阻塞 Gateway 核心功能
pub struct GrpcAckPublisher {
    /// Push/Session 服务名称（用于服务发现）
    service_name: String,
    /// gRPC 客户端（懒加载）
    client: Arc<Mutex<Option<flare_proto::push::push_service_client::PushServiceClient<tonic::transport::Channel>>>>,
    /// 服务发现客户端（懒加载）
    service_client: Arc<Mutex<Option<flare_server_core::discovery::ServiceClient>>>,
}

impl GrpcAckPublisher {
    /// 创建 gRPC ACK 发布器
    /// 
    /// # 参数
    /// * `service_name` - Push/Session 服务名称（用于服务发现）
    pub fn new(service_name: String) -> Arc<Self> {
        Arc::new(Self {
            service_name,
            client: Arc::new(Mutex::new(None)),
            service_client: Arc::new(Mutex::new(None)),
        })
    }
    
    /// 确保 gRPC 客户端已初始化（懒加载）- 暂时禁用
    #[allow(dead_code)]
    async fn ensure_client(
        &self,
    ) -> Result<flare_proto::push::push_service_client::PushServiceClient<tonic::transport::Channel>> {
        // TODO: 重构为正确的服务发现与客户端初始化
        Err(ErrorBuilder::new(
            ErrorCode::ServiceUnavailable,
            "Push service client temporarily disabled",
        )
        .build_error())
    }
}

#[async_trait]
impl AckPublisher for GrpcAckPublisher {
    async fn publish_ack(&self, event: &AckAuditEvent) -> Result<()> {
        // TODO: 重构为使用 PushAck 协议上报 ACK
        // 当前暂时只记录日志，不实际上报到 Push 服务
        debug!(
            message_id = %event.ack.message_id,
            user_id = %event.user_id,
            ack_type = %event.ack_type(),
            status = ?event.ack.status,
            "ACK event recorded (gRPC report disabled temporarily)"
        );
        Ok(())
    }
}

/// Noop ACK发布器（用于测试或禁用ACK上报）
pub struct NoopAckPublisher;

impl NoopAckPublisher {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

#[async_trait]
impl AckPublisher for NoopAckPublisher {
    async fn publish_ack(&self, _event: &AckAuditEvent) -> Result<()> {
        // 不做任何操作，用于测试或禁用 ACK 上报
        Ok(())
    }
}

