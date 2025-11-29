//! # Gateway命令处理器（编排层）
//!
//! 负责处理命令，调用领域服务

// 推送命令
mod push_commands {
    use std::sync::Arc;
    use std::time::Instant;
    use chrono::Utc;

    use flare_proto::access_gateway::{
        BatchPushMessageRequest, BatchPushMessageResponse, BatchPushOptions, BatchPushStatistics,
        PushMessageRequest, PushMessageResponse, PushOptions, PushResult, PushStatistics, PushStatus,
    };
    use tonic::Status;
    use anyhow::Result;
    use tracing::{error, instrument, warn, Span};
    use flare_proto::RpcStatus;
    use flare_proto::common::ErrorCode;

    // Helper function to create OK RpcStatus
    fn ok_status() -> RpcStatus {
        RpcStatus {
            code: ErrorCode::Ok as i32,
            message: "".to_string(),
            details: vec![],
            context: None,
        }
    }
    use prost::Message;

    use crate::domain::service::{DomainPushResult, PushDomainService};
    use crate::infrastructure::AckPublisher;

    /// 推送消息命令
    #[derive(Debug)]
    pub struct PushMessageCommand {
        pub request: PushMessageRequest,
    }

    /// 批量推送消息命令
    pub struct BatchPushMessageCommand {
        pub request: BatchPushMessageRequest,
    }

    /// 推送消息服务（应用层 - 编排领域服务并记录指标）
    pub struct PushMessageService {
        domain_service: Arc<PushDomainService>,
        ack_publisher: Option<Arc<dyn AckPublisher>>,
        gateway_id: String,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    }

    impl PushMessageService {
        pub fn new(
            domain_service: Arc<PushDomainService>,
            ack_publisher: Option<Arc<dyn AckPublisher>>,
            gateway_id: String,
            metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
        ) -> Self {
            Self {
                domain_service,
                ack_publisher,
                gateway_id,
                metrics,
            }
        }

        /// 处理推送消息请求
        pub async fn handle_push_message(
            &self,
            command: PushMessageCommand,
        ) -> Result<PushMessageResponse> {
            let start_time = Instant::now();
            let request = command.request;
            let target_user_ids = request.target_user_ids;
            let message = request.message.ok_or_else(|| {
                Status::invalid_argument("message is required".to_string())
            })?;
            let options = request.options.unwrap_or_default();
            let span = Span::current();

            // 提取 tenant_id（如果消息中有）
            let tenant_id = message.extra.get("tenant_id")
                .cloned()
                .unwrap_or_else(|| "unknown".to_string());
            
            // 设置追踪属性
            #[cfg(feature = "tracing")]
            {
                use flare_im_core::tracing::{set_tenant_id, set_message_id};
                set_tenant_id(&span, &tenant_id);
                if !message.id.is_empty() {
                    set_message_id(&span, &message.id);
                    span.record("message_id", &message.id);
                }
                span.record("user_count", target_user_ids.len() as u64);
            }

            let mut results = Vec::new();
            let mut total_users = 0;
            let mut online_users = 0;
            let mut offline_users = 0;
            let mut success_count = 0;
            let mut failure_count = 0;

            // 序列化消息
            let message_bytes = message.encode_to_vec();

            // 处理每个目标用户
            for user_id in target_user_ids {
                total_users += 1;

                // 检查用户是否在线（使用领域服务）
                let is_online = match self.domain_service.check_user_online(&user_id).await {
                    Ok(online) => online,
                    Err(e) => {
                        tracing::warn!(error = %e, user_id = %user_id, "Failed to check user online status");
                        false
                    }
                };

                if !is_online {
                    offline_users += 1;
                    results.push(PushResult {
                        user_id: user_id.clone(),
                        status: PushStatus::UserOffline as i32,
                        success_count: 0,
                        failure_count: 0,
                        error_message: "User is offline".to_string(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });
                    continue;
                }

                online_users += 1;

                // 获取过滤后的连接（使用领域服务）
                let filtered_connections = match self.domain_service.get_filtered_connections(&user_id, &options).await {
                    Ok(conns) => conns,
                    Err(e) => {
                        tracing::warn!(error = %e, user_id = %user_id, "Failed to get filtered connections");
                        offline_users += 1;
                        results.push(PushResult {
                            user_id: user_id.clone(),
                            status: PushStatus::Failed as i32,
                            success_count: 0,
                            failure_count: 0,
                            error_message: format!("Failed to get connections: {}", e),
                            pushed_at: Some(prost_types::Timestamp {
                                seconds: Utc::now().timestamp(),
                                nanos: 0,
                            }),
                        });
                        continue;
                    }
                };

                if filtered_connections.is_empty() {
                    offline_users += 1;
                    results.push(PushResult {
                        user_id: user_id.clone(),
                        status: PushStatus::UserOffline as i32,
                        success_count: 0,
                        failure_count: 0,
                        error_message: "No matching connections".to_string(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });
                    continue;
                }

                // 推送消息到所有匹配的连接（使用领域服务）
                let push_start = Instant::now();
                let domain_result = match self.domain_service.push_to_connections(&user_id, &filtered_connections, &message_bytes).await {
                    Ok((user_success, user_failure)) => {
                        success_count += user_success;
                        failure_count += user_failure;
                        DomainPushResult {
                            user_id: user_id.clone(),
                            success_count: user_success,
                            failure_count: user_failure,
                            error_message: if user_failure > 0 {
                                format!("Failed to push to {} connections", user_failure)
                            } else {
                                String::new()
                            },
                        }
                    }
                    Err(e) => {
                        failure_count += filtered_connections.len() as i32;
                        DomainPushResult {
                            user_id: user_id.clone(),
                            success_count: 0,
                            failure_count: filtered_connections.len() as i32,
                            error_message: format!("Push failed: {}", e),
                        }
                    }
                };

                // 记录推送延迟（应用层关注点）
                let push_duration = push_start.elapsed();
                self.metrics.push_latency_seconds
                    .with_label_values(&[&tenant_id])
                    .observe(push_duration.as_secs_f64());

                // 记录推送成功/失败指标（应用层关注点）
                if domain_result.success_count > 0 {
                    self.metrics.push_success_total
                        .with_label_values(&[&tenant_id])
                        .inc();
                    self.metrics.messages_pushed_total
                        .with_label_values(&[&tenant_id])
                        .inc();
                }
                if domain_result.failure_count > 0 {
                    self.metrics.push_failure_total
                        .with_label_values(&["push_error", &tenant_id])
                        .inc();
                }

                // 发布 ACK 事件（应用层关注点）
                if let Some(ref ack_publisher) = self.ack_publisher {
                    for conn in &filtered_connections {
                        let ack_status = if domain_result.success_count > 0 && domain_result.failure_count == 0 {
                            "success"
                        } else {
                            "failed"
                        };
                        
                        let ack_event = crate::infrastructure::PushAckEvent {
                            message_id: message.id.clone(),
                            user_id: user_id.clone(),
                            connection_id: conn.connection_id.clone(),
                            gateway_id: self.gateway_id.clone(),
                            ack_type: "push_ack".to_string(),
                            status: ack_status.to_string(),
                            timestamp: Utc::now().timestamp(),
                        };
                        
                        if let Err(e) = ack_publisher.publish_ack(&ack_event).await {
                            tracing::warn!(
                                error = %e,
                                message_id = %message.id,
                                user_id = %user_id,
                                "Failed to publish push ACK"
                            );
                        }
                    }
                }

                // 确定推送状态
                let push_status = if domain_result.success_count > 0 && domain_result.failure_count == 0 {
                    PushStatus::Success as i32
                } else if domain_result.success_count > 0 && domain_result.failure_count > 0 {
                    PushStatus::Partial as i32
                } else {
                    PushStatus::Failed as i32
                };

                results.push(PushResult {
                    user_id: domain_result.user_id,
                    status: push_status,
                    success_count: domain_result.success_count,
                    failure_count: domain_result.failure_count,
                    error_message: domain_result.error_message,
                    pushed_at: Some(prost_types::Timestamp {
                        seconds: Utc::now().timestamp(),
                        nanos: 0,
                    }),
                });
            }

            let statistics = PushStatistics {
                total_users,
                online_users,
                offline_users,
                success_count,
                failure_count,
            };

            // 记录总耗时
            let total_duration = start_time.elapsed();
            self.metrics.push_latency_seconds
                .with_label_values(&[&tenant_id])
                .observe(total_duration.as_secs_f64());

            Ok(PushMessageResponse {
                results,
                status: Some(ok_status()),
                statistics: Some(statistics),
            })
        }

        /// 处理批量推送消息请求
        pub async fn handle_batch_push_message(
            &self,
            command: BatchPushMessageCommand,
        ) -> Result<BatchPushMessageResponse> {
            let request = command.request;
            let pushes = request.pushes;
            let pushes_len = pushes.len(); // 保存长度，因为后面会被移动
            let options = request.options.unwrap_or_default();

            let max_concurrency = options.max_concurrency.max(1).min(1000) as usize;
            let parallel = options.parallel;

            let mut results = Vec::new();
            let mut total_tasks: i32 = 0;
            let mut success_tasks: i32 = 0;
            let mut failure_tasks: i32 = 0;
            let mut total_users: i32 = 0;
            let mut success_users: i32 = 0;
            let mut failure_users: i32 = 0;

            if parallel {
                // 并行推送
                use futures::stream::{self, StreamExt};
                let push_futures: Vec<_> = pushes
                    .into_iter()
                    .map(|push| {
                        let service = self.clone_service();
                        async move {
                            service
                                .handle_push_message(PushMessageCommand { request: push })
                                .await
                        }
                    })
                    .collect();

                let mut stream = stream::iter(push_futures)
                    .buffer_unordered(max_concurrency)
                    .enumerate();

                while let Some((idx, result)) = stream.next().await {
                    total_tasks += 1;
                    let should_break = self.process_batch_push_result(
                        result,
                        idx,
                        pushes_len,
                        &mut results,
                        &mut success_tasks,
                        &mut failure_tasks,
                        &mut total_users,
                        &mut success_users,
                        &mut failure_users,
                        &options,
                    );
                    if should_break {
                        break;
                    }
                }
            } else {
                // 串行推送
                for push in pushes {
                    total_tasks += 1;
                    let should_break = self.process_batch_push_result(
                        self.handle_push_message(PushMessageCommand { request: push }).await,
                        0, // 串行模式下不使用索引
                        pushes_len,
                        &mut results,
                        &mut success_tasks,
                        &mut failure_tasks,
                        &mut total_users,
                        &mut success_users,
                        &mut failure_users,
                        &options,
                    );
                    if should_break {
                        break;
                    }
                }
            }

            let statistics = BatchPushStatistics {
                total_tasks,
                success_tasks,
                failure_tasks,
                total_users,
                success_users,
                failure_users,
            };

            Ok(BatchPushMessageResponse {
                results,
                status: Some(ok_status()),
                statistics: Some(statistics),
            })
        }

        fn clone_service(&self) -> Self {
            Self {
                domain_service: Arc::clone(&self.domain_service),
                ack_publisher: self.ack_publisher.clone(),
                gateway_id: self.gateway_id.clone(),
                metrics: Arc::clone(&self.metrics),
            }
        }

        /// 处理批量推送结果（内部辅助函数）
        /// 
        /// 提取统计信息收集和错误处理逻辑，减少代码重复
        /// 返回是否应该中断处理（fail_fast 模式）
        fn process_batch_push_result(
            &self,
            result: Result<PushMessageResponse>,
            task_index: usize,
            total_tasks: usize,
            results: &mut Vec<PushMessageResponse>,
            success_tasks: &mut i32,
            failure_tasks: &mut i32,
            total_users: &mut i32,
            success_users: &mut i32,
            failure_users: &mut i32,
            options: &BatchPushOptions,
        ) -> bool {
            match result {
                Ok(response) => {
                    *success_tasks += 1;
                    if let Some(ref stats) = response.statistics {
                        *total_users += stats.total_users;
                        *success_users += stats.success_count;
                        *failure_users += stats.failure_count;
                    }
                    results.push(response);

                    // 检查 fail_fast 条件
                    if options.fail_fast && *failure_users > 0 {
                        if task_index > 0 {
                            // 并行模式下，计算剩余任务数
                            *failure_tasks += (total_tasks - task_index - 1) as i32;
                        }
                        return true;
                    }
                    false
                }
                Err(err) => {
                    // 立即转换为 String，避免 Send 问题
                    let error_msg = err.to_string();
                    
                    *failure_tasks += 1;
                    *failure_users += 1;
                    tracing::error!(
                        error = %error_msg,
                        task_index = task_index,
                        "Failed to process batch push task"
                    );

                    if options.fail_fast {
                        if task_index > 0 {
                            // 并行模式下，计算剩余任务数
                            *failure_tasks += (total_tasks - task_index - 1) as i32;
                        }
                        return true;
                    }

                    // 返回错误响应
                    use flare_server_core::error::{to_rpc_status, ErrorBuilder, ErrorCode};
                    let flare_error = ErrorBuilder::new(ErrorCode::InternalError, error_msg)
                        .build_error();
                    let error_status = to_rpc_status(&flare_error);
                    results.push(PushMessageResponse {
                        results: vec![],
                        status: Some(error_status),
                        statistics: None,
                    });
                    false
                }
            }
        }
    }

    impl Clone for PushMessageService {
        fn clone(&self) -> Self {
            self.clone_service()
        }
    }
}

// 会话命令
mod session_commands {
    use std::sync::Arc;

    use flare_proto::signaling::{
        HeartbeatRequest, HeartbeatResponse, LoginRequest, LoginResponse, LogoutRequest, LogoutResponse,
    };
    use flare_server_core::error::{ErrorBuilder, ErrorCode, Result, ok_status, to_rpc_status};
    use tracing::{info, warn};

    use crate::domain::model::Session;
use crate::domain::repository::{SessionStore, SignalingGateway};

    pub struct LoginCommand {
        pub request: LoginRequest,
    }

    pub struct LogoutCommand {
        pub request: LogoutRequest,
    }

    pub struct HeartbeatCommand {
        pub request: HeartbeatRequest,
    }

    pub struct SessionCommandService {
        signaling: Arc<dyn SignalingGateway>,
        store: Arc<dyn SessionStore>,
        gateway_id: String,
    }

    impl SessionCommandService {
        pub fn new(
            signaling: Arc<dyn SignalingGateway>,
            store: Arc<dyn SessionStore>,
            gateway_id: String,
        ) -> Self {
            Self {
                signaling,
                store,
                gateway_id,
            }
        }

        pub async fn handle_login(&self, command: LoginCommand) -> Result<LoginResponse> {
            let mut response = self.signaling.login(command.request.clone()).await?;

            if response.success {
                let session = Session::new(
                    response.session_id.clone(),
                    command.request.user_id,
                    command.request.device_id,
                    Some(response.route_server.clone()),
                    self.gateway_id.clone(),
                );
                self.store.insert(session).await?;
                info!(session_id = %response.session_id, "session registered");
                if response.status.is_none() {
                    response.status = Some(ok_status());
                }
            } else {
                warn!(error = %response.error_message, "login failed via signaling");
                if response.status.is_none() {
                    let error =
                        ErrorBuilder::new(ErrorCode::AuthenticationFailed, "signaling login rejected")
                            .details(response.error_message.clone())
                            .build_error();
                    response.status = Some(to_rpc_status(&error));
                }
            }

            Ok(response)
        }

        pub async fn handle_logout(
            &self,
            command: LogoutCommand,
        ) -> Result<(LogoutResponse, Option<Session>)> {
            let mut response = self.signaling.logout(command.request.clone()).await?;

            let removed = if response.success {
                let removed = self.store.remove(&command.request.session_id).await?;
                if let Some(ref session) = removed {
                    info!(session_id = %session.session_id, user_id = %session.user_id, "session removed");
                }
                removed
            } else {
                None
            };

            if response.status.is_none() {
                if response.success {
                    response.status = Some(ok_status());
                } else {
                    let error =
                        ErrorBuilder::new(ErrorCode::OperationFailed, "logout failed via signaling")
                            .build_error();
                    response.status = Some(to_rpc_status(&error));
                }
            }

            Ok((response, removed))
        }

        pub async fn handle_heartbeat(&self, command: HeartbeatCommand) -> Result<HeartbeatResponse> {
            let mut response = self.signaling.heartbeat(command.request.clone()).await?;

            if response.success {
                let _ = self.store.touch(&command.request.session_id).await?;
                if response.status.is_none() {
                    response.status = Some(ok_status());
                }
            } else if response.status.is_none() {
                let error = ErrorBuilder::new(
                    ErrorCode::OperationFailed,
                    "heartbeat rejected by signaling",
                )
                .build_error();
                response.status = Some(to_rpc_status(&error));
            }

            Ok(response)
        }

        pub fn store(&self) -> Arc<dyn SessionStore> {
            self.store.clone()
        }
    }
}

// 导出
pub use push_commands::{BatchPushMessageCommand, PushMessageCommand, PushMessageService};
pub use session_commands::{
    HeartbeatCommand, LoginCommand, LogoutCommand, SessionCommandService,
};

