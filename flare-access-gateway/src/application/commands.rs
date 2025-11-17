//! 应用层命令模块
//!
//! 处理业务系统的命令操作

// 推送命令
mod push_commands {
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH, Instant};

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

    use crate::domain::repositories::ConnectionQuery;
    use crate::infrastructure::online_cache::OnlineStatusCache;
    use crate::interface::connection::LongConnectionHandler;

    /// 推送消息命令
    #[derive(Debug)]
    pub struct PushMessageCommand {
        pub request: PushMessageRequest,
    }

    /// 批量推送消息命令
    pub struct BatchPushMessageCommand {
        pub request: BatchPushMessageRequest,
    }

    /// 推送消息服务
    pub struct PushMessageService {
        connection_handler: Arc<LongConnectionHandler>,
        connection_query: Arc<dyn ConnectionQuery>,
        online_cache: Arc<OnlineStatusCache>,
        ack_publisher: Option<Arc<dyn crate::infrastructure::AckPublisher>>,
        gateway_id: String,
        metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
    }

    impl PushMessageService {
        pub fn new(
            connection_handler: Arc<LongConnectionHandler>,
            connection_query: Arc<dyn ConnectionQuery>,
            online_cache: Arc<OnlineStatusCache>,
            ack_publisher: Option<Arc<dyn crate::infrastructure::AckPublisher>>,
            gateway_id: String,
            metrics: Arc<flare_im_core::metrics::AccessGatewayMetrics>,
        ) -> Self {
            Self {
                connection_handler,
                connection_query,
                online_cache,
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

                // 1. 先检查本地缓存
                let is_online = if let Some((online, _gateway_id)) = self.online_cache.get(&user_id).await {
                    self.metrics.online_cache_hit_total.inc();
                    online
                } else {
                    self.metrics.online_cache_miss_total.inc();
                    // 2. 缓存未命中，查询连接管理器（本地内存查询，零延迟）
                    let connections = self
                        .connection_query
                        .query_user_connections(&user_id)
                        .await?;
                    
                    let is_online = !connections.is_empty();
                    
                    // 3. 更新缓存（即使离线也缓存，避免频繁查询）
                    if let Some(gateway_id) = connections.first().map(|_| "local".to_string()) {
                        self.online_cache
                            .set(user_id.clone(), gateway_id, is_online)
                            .await;
                    }
                    
                    is_online
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
                            seconds: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64,
                            nanos: 0,
                        }),
                    });
                    continue;
                }

                online_users += 1;

                // 查询连接（本地内存查询，零延迟）
                let connections = self
                    .connection_query
                    .query_user_connections(&user_id)
                    .await?;

                // 过滤连接（根据设备ID和平台）
                let filtered_connections = self.filter_connections(&connections, &options);

                if filtered_connections.is_empty() {
                    offline_users += 1;
                    results.push(PushResult {
                        user_id: user_id.clone(),
                        status: PushStatus::UserOffline as i32,
                        success_count: 0,
                        failure_count: 0,
                        error_message: "No matching connections".to_string(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64,
                            nanos: 0,
                        }),
                    });
                    continue;
                }

                // 推送消息到所有匹配的连接
                let mut user_success_count = 0;
                let mut user_failure_count = 0;
                let push_start = Instant::now();

                for conn in filtered_connections {
                    match self
                        .connection_handler
                        .push_message_to_connection(&conn.connection_id, message_bytes.clone())
                        .await
                    {
                        Ok(_) => {
                            user_success_count += 1;
                            success_count += 1;
                            
                            // 记录推送成功指标
                            self.metrics.push_success_total
                                .with_label_values(&[&tenant_id])
                                .inc();
                            self.metrics.messages_pushed_total
                                .with_label_values(&[&tenant_id])
                                .inc();
                            
                            // 上报推送ACK（消息成功推送到连接）
                            if let Some(ref ack_publisher) = self.ack_publisher {
                                let message_id = message.id.clone();
                                let ack_event = crate::infrastructure::PushAckEvent {
                                    message_id: message_id.clone(),
                                    user_id: user_id.clone(),
                                    connection_id: conn.connection_id.clone(),
                                    gateway_id: self.gateway_id.clone(),
                                    ack_type: "push_ack".to_string(),
                                    status: "success".to_string(),
                                    timestamp: SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs() as i64,
                                };
                                
                                if let Err(e) = ack_publisher.publish_ack(&ack_event).await {
                                    tracing::warn!(
                                        ?e,
                                        message_id = %message_id,
                                        user_id = %user_id,
                                        "Failed to publish push ACK"
                                    );
                                }
                            }
                        }
                        Err(err) => {
                            // 立即转换为 String，避免 Send 问题
                            let error_reason = err.to_string();
                            
                            user_failure_count += 1;
                            failure_count += 1;
                            
                            // 记录推送失败指标
                            self.metrics.push_failure_total
                                .with_label_values(&[&error_reason, &tenant_id])
                                .inc();
                            
                            tracing::warn!(
                                error = %error_reason,
                                user_id = %user_id,
                                connection_id = %conn.connection_id,
                                "Failed to push message to connection"
                            );
                            
                            // 上报推送失败ACK（在单独的作用域中，确保 err 不会被捕获）
                            {
                                let ack_publisher = self.ack_publisher.as_ref();
                                if let Some(ack_publisher) = ack_publisher {
                                    let message_id = message.id.clone();
                                    let ack_event = crate::infrastructure::PushAckEvent {
                                        message_id: message_id.clone(),
                                        user_id: user_id.clone(),
                                        connection_id: conn.connection_id.clone(),
                                        gateway_id: self.gateway_id.clone(),
                                        ack_type: "push_ack".to_string(),
                                        status: "failed".to_string(),
                                        timestamp: SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap()
                                            .as_secs() as i64,
                                    };
                                    
                                    let ack_result = ack_publisher.publish_ack(&ack_event).await;
                                    if let Err(e) = ack_result {
                                        tracing::warn!(
                                            error = %e,
                                            message_id = %message_id,
                                            user_id = %user_id,
                                            "Failed to publish push failure ACK"
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                // 记录推送延迟
                let push_duration = push_start.elapsed();
                self.metrics.push_latency_seconds
                    .with_label_values(&[&tenant_id])
                    .observe(push_duration.as_secs_f64());

                // 确定推送状态
                let push_status = if user_success_count > 0 && user_failure_count == 0 {
                    PushStatus::Success as i32
                } else if user_success_count > 0 && user_failure_count > 0 {
                    PushStatus::Partial as i32
                } else {
                    PushStatus::Failed as i32
                };

                results.push(PushResult {
                    user_id: user_id.clone(),
                    status: push_status,
                    success_count: user_success_count,
                    failure_count: user_failure_count,
                    error_message: if user_failure_count > 0 {
                        format!("Failed to push to {} connections", user_failure_count)
                    } else {
                        String::new()
                    },
                    pushed_at: Some(prost_types::Timestamp {
                        seconds: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs() as i64,
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
                    match result {
                        Ok(response) => {
                            success_tasks += 1;
                            total_users += response.statistics.as_ref().map(|s| s.total_users).unwrap_or(0);
                            success_users += response
                                .statistics
                                .as_ref()
                                .map(|s| s.success_count)
                                .unwrap_or(0);
                            failure_users += response
                                .statistics
                                .as_ref()
                                .map(|s| s.failure_count)
                                .unwrap_or(0);
                            results.push(response);

                            if options.fail_fast && failure_users > 0 {
                                failure_tasks += (pushes_len - idx - 1) as i32;
                                break;
                            }
                        }
                        Err(err) => {
                            // 立即转换为 String，避免 Send 问题
                            let error_msg = err.to_string();
                            
                            failure_tasks += 1;
                            failure_users += 1;
                            tracing::error!(error = %error_msg, task_index = idx, "Failed to process batch push task");

                            if options.fail_fast {
                                failure_tasks += (pushes_len - idx - 1) as i32;
                                break;
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
                        }
                    }
                }
            } else {
                // 串行推送
                for push in pushes {
                    total_tasks += 1;
                    match self
                        .handle_push_message(PushMessageCommand {
                            request: push,
                        })
                        .await
                    {
                        Ok(response) => {
                            success_tasks += 1;
                            total_users += response.statistics.as_ref().map(|s| s.total_users).unwrap_or(0);
                            success_users += response
                                .statistics
                                .as_ref()
                                .map(|s| s.success_count)
                                .unwrap_or(0);
                            failure_users += response
                                .statistics
                                .as_ref()
                                .map(|s| s.failure_count)
                                .unwrap_or(0);
                            results.push(response);

                            if options.fail_fast && failure_users > 0 {
                                break;
                            }
                        }
                        Err(err) => {
                            // 立即转换为 String，避免 Send 问题
                            let error_msg = err.to_string();
                            
                            failure_tasks += 1;
                            failure_users += 1;
                            tracing::error!(error = %error_msg, "Failed to process batch push task");

                            if options.fail_fast {
                                break;
                            }

                            use flare_server_core::error::{to_rpc_status, ErrorBuilder, ErrorCode};
                            let flare_error = ErrorBuilder::new(ErrorCode::InternalError, error_msg)
                                .build_error();
                            let error_status = to_rpc_status(&flare_error);
                            results.push(PushMessageResponse {
                                results: vec![],
                                status: Some(error_status),
                                statistics: None,
                            });
                        }
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

        /// 过滤连接（根据设备ID和平台）
        fn filter_connections(
            &self,
            connections: &[crate::domain::models::ConnectionInfo],
            options: &PushOptions,
        ) -> Vec<crate::domain::models::ConnectionInfo> {
            let mut filtered = connections.to_vec();

            // 过滤设备ID
            if !options.device_ids.is_empty() {
                filtered.retain(|conn| options.device_ids.contains(&conn.device_id));
            }

            // 过滤平台
            if !options.platforms.is_empty() {
                filtered.retain(|conn| options.platforms.contains(&conn.platform));
            }

            filtered
        }

        fn clone_service(&self) -> Self {
            Self {
                connection_handler: Arc::clone(&self.connection_handler),
                connection_query: Arc::clone(&self.connection_query),
                online_cache: Arc::clone(&self.online_cache),
                ack_publisher: self.ack_publisher.clone(),
                gateway_id: self.gateway_id.clone(),
                metrics: Arc::clone(&self.metrics),
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

    use crate::domain::models::Session;
use crate::domain::repositories::{SessionStore, SignalingGateway};

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

