//! 统一异常处理模块

use thiserror::Error;

/// 推送服务错误类型
#[derive(Debug, Error)]
pub enum PushServerError {
    /// 配置错误
    #[error("Configuration error: {0}")]
    Config(String),
    
    /// 服务不可用
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),
    
    /// 消息处理错误
    #[error("Message processing error: {0}")]
    MessageProcessing(String),
    
    /// 在线状态查询错误
    #[error("Online status query error: {0}")]
    OnlineStatusQuery(String),
    
    /// 网关路由错误
    #[error("Gateway routing error: {0}")]
    GatewayRouting(String),
}

// Note: flare_server_core::error::Error 类型不存在，使用 Result 类型
// 如果需要转换，可以通过 ErrorBuilder 构建错误

impl From<anyhow::Error> for PushServerError {
    fn from(err: anyhow::Error) -> Self {
        PushServerError::MessageProcessing(err.to_string())
    }
}

