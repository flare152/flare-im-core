//! 命令结构体定义（Command DTO）

use flare_proto::access_gateway::{PublishSignalRequest, SubscribeRequest, UnsubscribeRequest};
use flare_proto::signaling::online::{HeartbeatRequest, LoginRequest, LogoutRequest};

/// 登录命令
#[derive(Debug, Clone)]
pub struct LoginCommand {
    /// 原始请求
    pub request: LoginRequest,
}

/// 登出命令
#[derive(Debug, Clone)]
pub struct LogoutCommand {
    /// 原始请求
    pub request: LogoutRequest,
}

/// 心跳命令
#[derive(Debug, Clone)]
pub struct HeartbeatCommand {
    /// 原始请求
    pub request: HeartbeatRequest,
}

/// 订阅命令
#[derive(Debug, Clone)]
pub struct SubscribeCommand {
    /// 原始请求
    pub request: SubscribeRequest,
}

/// 取消订阅命令
#[derive(Debug, Clone)]
pub struct UnsubscribeCommand {
    /// 原始请求
    pub request: UnsubscribeRequest,
}

/// 发布信号命令
#[derive(Debug, Clone)]
pub struct PublishSignalCommand {
    /// 原始请求
    pub request: PublishSignalRequest,
}
