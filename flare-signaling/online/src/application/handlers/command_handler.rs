//! 命令处理器（编排层）- 轻量级，只负责编排领域服务

use std::sync::Arc;

use anyhow::Result;
use flare_proto::access_gateway::{PublishSignalResponse, SubscribeResponse, UnsubscribeResponse};
use flare_proto::signaling::online::{HeartbeatResponse, LoginResponse, LogoutResponse};
use tracing::instrument;

use crate::application::commands::{
    HeartbeatCommand, LoginCommand, LogoutCommand, PublishSignalCommand, SubscribeCommand,
    UnsubscribeCommand,
};
use crate::domain::service::{OnlineStatusDomainService, SubscriptionDomainService};

/// 在线状态命令处理器（编排层）
pub struct OnlineCommandHandler {
    online_domain_service: Arc<OnlineStatusDomainService>,
    subscription_domain_service: Arc<SubscriptionDomainService>,
}

impl OnlineCommandHandler {
    pub fn new(
        online_domain_service: Arc<OnlineStatusDomainService>,
        subscription_domain_service: Arc<SubscriptionDomainService>,
    ) -> Self {
        Self {
            online_domain_service,
            subscription_domain_service,
        }
    }

    /// 处理登录命令
    #[instrument(skip(self), fields(user_id = %command.request.user_id, device_id = %command.request.device_id))]
    pub async fn handle_login(&self, command: LoginCommand) -> Result<LoginResponse> {
        self.online_domain_service.login(command.request).await
    }

    /// 处理登出命令
    #[instrument(skip(self), fields(user_id = %command.request.user_id, session_id = %command.request.session_id))]
    pub async fn handle_logout(&self, command: LogoutCommand) -> Result<LogoutResponse> {
        self.online_domain_service.logout(command.request).await
    }

    /// 处理心跳命令
    #[instrument(skip(self), fields(session_id = %command.request.session_id, user_id = %command.request.user_id))]
    pub async fn handle_heartbeat(&self, command: HeartbeatCommand) -> Result<HeartbeatResponse> {
        self.online_domain_service
            .heartbeat(
                &command.request.session_id,
                &command.request.user_id,
                command.request.current_quality.as_ref(),
            )
            .await
    }

    /// 处理订阅命令
    #[instrument(skip(self), fields(user_id = %command.request.user_id))]
    pub async fn handle_subscribe(&self, command: SubscribeCommand) -> Result<SubscribeResponse> {
        self.subscription_domain_service
            .subscribe(command.request)
            .await
    }

    /// 处理取消订阅命令
    #[instrument(skip(self), fields(user_id = %command.request.user_id))]
    pub async fn handle_unsubscribe(
        &self,
        command: UnsubscribeCommand,
    ) -> Result<UnsubscribeResponse> {
        self.subscription_domain_service
            .unsubscribe(command.request)
            .await
    }

    /// 处理发布信号命令
    #[instrument(skip(self))]
    pub async fn handle_publish_signal(
        &self,
        command: PublishSignalCommand,
    ) -> Result<PublishSignalResponse> {
        self.subscription_domain_service
            .publish_signal(command.request)
            .await
    }
}
