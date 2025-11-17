//! 应用层命令模块

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use flare_im_core::hooks::HookDispatcher;
use flare_im_core::metrics::PushServerMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_user_id, set_gateway_id, set_push_type, set_tenant_id, set_error};
use flare_proto::push::{PushMessageRequest, PushNotificationRequest, PushOptions};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use prost::Message;
use tracing::{info, warn, instrument, Span};

use crate::config::PushServerConfig;
use crate::domain::models::{DispatchNotification, PushDispatchTask, RequestMetadata};
use crate::domain::repositories::{OnlineStatusRepository, PushTaskPublisher};
use crate::infrastructure::ack_tracker::{AckTracker, AckType};
use crate::infrastructure::gateway_router::GatewayRouterTrait;
use crate::infrastructure::message_state::{MessageStateTracker, MessageStatus};
use crate::infrastructure::retry::{execute_with_retry, RetryPolicy};
use crate::hooks::{
    build_post_send_record, finalize_notification, prepare_message_envelope,
    prepare_notification_envelope,
};

pub struct PushDispatchCommandService {
    config: Arc<PushServerConfig>,
    online_repo: Arc<dyn OnlineStatusRepository>,
    task_publisher: Arc<dyn PushTaskPublisher>,
    hooks: Arc<HookDispatcher>,
    gateway_router: Arc<dyn GatewayRouterTrait>,
    state_tracker: Arc<MessageStateTracker>,
    ack_tracker: Arc<AckTracker>,
    retry_policy: RetryPolicy,
    metrics: Arc<PushServerMetrics>,
}

impl PushDispatchCommandService {
    pub fn new(
        config: Arc<PushServerConfig>,
        online_repo: Arc<dyn OnlineStatusRepository>,
        task_publisher: Arc<dyn PushTaskPublisher>,
        hooks: Arc<HookDispatcher>,
        gateway_router: Arc<dyn GatewayRouterTrait>,
        state_tracker: Arc<MessageStateTracker>,
        ack_tracker: Arc<AckTracker>,
        metrics: Arc<PushServerMetrics>,
    ) -> Self {
        let retry_policy = RetryPolicy::from_config(
            config.push_retry_max_attempts,
            config.push_retry_initial_delay_ms,
            config.push_retry_max_delay_ms,
            config.push_retry_backoff_multiplier,
        );

        Self {
            config,
            online_repo,
            task_publisher,
            hooks,
            gateway_router,
            state_tracker,
            ack_tracker,
            retry_policy,
            metrics,
        }
    }
    
    /// 批量处理推送任务（从Kafka消费的批量消息）
    pub async fn handle_push_batch(&self, batch: Vec<Vec<u8>>) -> Result<()> {
        use prost::Message;
        use serde_json;
        
        // 解析批量消息为PushTask
        let mut push_tasks = Vec::new();
        for payload in batch {
            // 尝试解析为PushMessageRequest或PushNotificationRequest
            // 注意：根据设计文档，Message Orchestrator生产的是PushTask格式
            // 这里先尝试解析为现有的格式，后续可以根据实际消息格式调整
            
            if let Ok(request) = PushMessageRequest::decode(payload.as_slice()) {
                // 转换为PushTask并添加到批量列表
                // 这里需要将PushMessageRequest转换为PushTask
                // 暂时先调用handle_push_message处理
                if let Err(err) = self.handle_push_message(request).await {
                    warn!(?err, "failed to process push message in batch");
                }
            } else if let Ok(request) = PushNotificationRequest::decode(payload.as_slice()) {
                if let Err(err) = self.handle_push_notification(request).await {
                    warn!(?err, "failed to process push notification in batch");
                }
            } else {
                // 尝试JSON格式（PushTask）
                if let Ok(task) = serde_json::from_slice::<PushDispatchTask>(&payload) {
                    push_tasks.push(task);
                } else {
                    warn!("unknown payload format in push batch");
                }
            }
        }
        
        // 如果有PushTask格式的消息，批量处理
        if !push_tasks.is_empty() {
            self.process_push_tasks_batch(push_tasks).await?;
        }
        
        Ok(())
    }
    
    /// 批量处理PushTask（核心批量处理逻辑）
    #[instrument(skip(self), fields(batch_size, tenant_id, message_type))]
    async fn process_push_tasks_batch(&self, tasks: Vec<PushDispatchTask>) -> Result<()> {
        let batch_start = Instant::now();
        let batch_size = tasks.len() as f64;
        let span = Span::current();
        
        // 记录批量大小
        self.metrics.batch_size.observe(batch_size);
        
        // 提取租户ID和消息类型用于指标
        let tenant_id = tasks.first()
            .and_then(|t| t.tenant_id.as_deref())
            .unwrap_or("unknown")
            .to_string();
        
        let message_type = tasks.first()
            .map(|t| t.message_type.as_str())
            .unwrap_or("unknown");
        
        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            set_tenant_id(&span, &tenant_id);
            span.record("batch_size", batch_size as u64);
            span.record("message_type", message_type);
        }
        
        // 记录处理总数
        self.metrics.push_tasks_processed_total
            .with_label_values(&[message_type, &tenant_id])
            .inc();
        
        // 1. 收集所有用户ID并设置消息类型
        // 检查是否是聊天室消息（receiver_ids 为空或 session_type="group"）
        let is_chatroom_message = tasks.first()
            .map(|t| {
                t.metadata.get("chatroom").map(|v| v == "true").unwrap_or(false)
                    || t.metadata.get("session_type").map(|v| v == "group").unwrap_or(false)
            })
            .unwrap_or(false);
        
        let user_ids: Vec<String> = if is_chatroom_message {
            // 聊天室消息：需要查询所有在线用户
            // 这里先使用 tasks 中的 user_id（发送者），后续会查询所有在线用户
            tasks
                .iter()
                .map(|t| t.user_id.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        } else {
            // 普通消息：使用 tasks 中的 user_id
            tasks
                .iter()
                .map(|t| t.user_id.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect()
        };
        
        // 设置消息状态和类型
        for task in &tasks {
            self.state_tracker
                .update_status(
                    &task.message_id,
                    &task.user_id,
                    MessageStatus::Pending,
                    None,
                )
                .await;
            self.state_tracker
                .set_message_type(&task.message_id, &task.user_id, task.message_type.clone())
                .await;
        }
        
        // 2. 批量查询在线状态
        #[cfg(feature = "tracing")]
        let query_span = create_span("push-server", "query_online_status");
        
        let query_start = Instant::now();
        let online_statuses = if is_chatroom_message {
            // 聊天室消息：查询所有在线用户
            info!(
                tenant_id = %tenant_id,
                "Chatroom message detected, querying all online users"
            );
            match self.online_repo.get_all_online_users(Some(tenant_id.as_str())).await {
                Ok(all_users) => {
                    info!(
                        tenant_id = %tenant_id,
                        count = all_users.len(),
                        "Found {} online users for chatroom broadcast",
                        all_users.len()
                    );
                    all_users
                }
                Err(err) => {
                    warn!(
                        error = %err,
                        tenant_id = %tenant_id,
                        "Failed to query all online users for chatroom message, falling back to sender only"
                    );
                    // 降级：只查询发送者
                    self.online_repo
                        .batch_get_online_status(&user_ids)
                        .await?
                }
            }
        } else {
            // 普通消息：查询指定用户的在线状态
            self.online_repo
                .batch_get_online_status(&user_ids)
                .await?
        };
        let query_duration = query_start.elapsed();
        self.metrics.online_status_query_duration_seconds.observe(query_duration.as_secs_f64());
        
        #[cfg(feature = "tracing")]
        query_span.end();
        
        // 如果是聊天室消息，需要为所有在线用户创建 PushTask
        let tasks_to_process: Vec<PushDispatchTask> = if is_chatroom_message {
            info!(
                tenant_id = %tenant_id,
                message_type = message_type,
                online_users_count = online_statuses.len(),
                "Processing chatroom message, will broadcast to all online users"
            );
            
            // 为所有在线用户创建 PushTask（排除发送者）
            let sender_ids: std::collections::HashSet<String> = tasks.iter()
                .map(|t| t.user_id.clone())
                .collect();
            
            let mut all_tasks = tasks.clone();
            for (user_id, status) in &online_statuses {
                // 跳过发送者（发送者不需要收到自己的消息）
                if !sender_ids.contains(user_id) && status.online {
                    // 从第一个任务克隆并修改 user_id
                    if let Some(base_task) = tasks.first() {
                        let mut new_task = base_task.clone();
                        new_task.user_id = user_id.clone();
                        new_task.online = true;
                        all_tasks.push(new_task);
                    }
                }
            }
            all_tasks
        } else {
            tasks.clone()
        };
        
        // 3. 按gateway_id分组在线用户
        let mut gateway_groups: HashMap<String, Vec<PushDispatchTask>> = HashMap::new();
        let mut offline_tasks = Vec::new();
        
        for mut task in tasks_to_process {
            if let Some(status) = online_statuses.get(&task.user_id) {
                task.online = status.online;
                
                if status.online {
                    // 在线用户：按gateway_id分组
                    if let Some(gateway_id) = &status.gateway_id {
                        info!(
                            user_id = %task.user_id,
                            gateway_id = %gateway_id,
                            "Adding user to gateway group"
                        );
                        gateway_groups
                            .entry(gateway_id.clone())
                            .or_insert_with(Vec::new)
                            .push(task);
                    } else {
                        warn!(
                            user_id = %task.user_id,
                            "User online but no gateway_id, falling back to offline push"
                        );
                        // 没有gateway_id，降级到离线推送
                        offline_tasks.push(task);
                    }
                } else {
                    // 离线用户
                    offline_tasks.push(task);
                }
            } else {
                warn!(
                    user_id = %task.user_id,
                    "User not found in online_statuses, falling back to offline push"
                );
                // 查询失败，降级到离线推送
                task.online = false;
                offline_tasks.push(task);
            }
        }
        
        info!(
            gateway_groups_count = gateway_groups.len(),
            offline_tasks_count = offline_tasks.len(),
            "Grouped tasks: {} gateways, {} offline tasks",
            gateway_groups.len(),
            offline_tasks.len()
        );
        
        // 4. 并发推送到多个网关（在线用户，带重试机制）
        info!(
            gateway_count = gateway_groups.len(),
            "Preparing to push to {} gateways",
            gateway_groups.len()
        );
        
        // 建立gateway_id到tasks的映射，用于后续结果处理
        let mut gateway_tasks_map: HashMap<String, Vec<PushDispatchTask>> = HashMap::new();
        
        let mut push_tasks = Vec::new();
        for (gateway_id, tasks) in gateway_groups {
            info!(
                gateway_id = %gateway_id,
                user_count = tasks.len(),
                "Preparing push to gateway"
            );
            // 保存gateway_id到tasks的映射
            gateway_tasks_map.insert(gateway_id.clone(), tasks.clone());
            let router = Arc::clone(&self.gateway_router);
            let state_tracker = Arc::clone(&self.state_tracker);
            let ack_tracker = Arc::clone(&self.ack_tracker);
            let retry_policy = self.retry_policy.clone();
            
            // 按用户ID分组，构建PushMessageRequest
            let mut user_ids = Vec::new();
            let mut message_payload: Option<Vec<u8>> = None;
            let mut tenant_context: Option<flare_proto::TenantContext> = None;
            let mut message_ids = Vec::new();
            let mut message_types = Vec::new();
            
            for task in &tasks {
                user_ids.push(task.user_id.clone());
                message_ids.push(task.message_id.clone());
                message_types.push(task.message_type.clone());
                if message_payload.is_none() {
                    message_payload = Some(task.message.clone());
                }
                if tenant_context.is_none() {
                    tenant_context = task.tenant_id.as_ref().map(|tid| {
                        flare_proto::TenantContext {
                            tenant_id: tid.clone(),
                            business_type: String::new(),
                            environment: String::new(),
                            organization_id: String::new(),
                            labels: HashMap::new(),
                            attributes: HashMap::new(),
                        }
                    });
                }
            }
            
            if !user_ids.is_empty() {
                // 构建access_gateway的PushMessageRequest
                use flare_proto::access_gateway::{PushMessageRequest as AccessPushRequest, PushOptions as AccessPushOptions};
                use flare_proto::storage::Message as StorageMessage;
                use prost_types::Timestamp;
                
                // 从第一个task中提取消息信息（所有task共享相同的消息内容）
                let first_task = &tasks[0];
                
                // 尝试从message payload中解析StorageMessage（如果payload是protobuf格式）
                let storage_message = if let Ok(parsed_msg) = StorageMessage::decode(first_task.message.as_slice()) {
                    // 如果成功解析，使用解析后的消息，但更新必要的字段
                    StorageMessage {
                        id: first_task.message_id.clone(),
                        receiver_ids: user_ids.clone(),
                        receiver_id: user_ids[0].clone(), // 使用第一个用户ID作为主接收者
                        tenant: tenant_context.clone(),
                        extra: {
                            let mut extra = parsed_msg.extra.clone();
                            // 确保消息类型在extra中
                            if !first_task.message_type.is_empty() {
                                extra.insert("message_type".to_string(), first_task.message_type.clone());
                            }
                            // 合并metadata到extra
                            for (k, v) in &first_task.metadata {
                                extra.insert(k.clone(), v.clone());
                            }
                            extra
                        },
                        ..parsed_msg
                    }
                } else {
                    // 如果解析失败，构建默认消息（从metadata中提取信息）
                    // 解析message_type为MessageType枚举值
                    let message_type_enum = match first_task.message_type.as_str() {
                        "text" | "text/plain" => flare_proto::storage::MessageType::Text as i32,
                        "binary" | "application/octet-stream" => flare_proto::storage::MessageType::Binary as i32,
                        "notification" => flare_proto::storage::MessageType::Custom as i32,
                        _ => flare_proto::storage::MessageType::Custom as i32,
                    };
                    
                    StorageMessage {
                        id: first_task.message_id.clone(),
                        session_id: first_task.metadata.get("session_id").cloned().unwrap_or_default(),
                        sender_id: first_task.metadata.get("sender_id").cloned().unwrap_or_default(),
                        sender_type: first_task.metadata.get("sender_type").cloned().unwrap_or_else(|| "user".to_string()),
                        receiver_ids: user_ids.clone(),
                        receiver_id: user_ids[0].clone(),
                        content: None, // 如果payload不是protobuf格式，content为空
                        timestamp: first_task.metadata.get("timestamp")
                            .and_then(|ts| ts.parse::<i64>().ok())
                            .map(|secs| Timestamp {
                                seconds: secs,
                                nanos: 0,
                            })
                            .or_else(|| Some(Timestamp {
                                seconds: chrono::Utc::now().timestamp(),
                                nanos: 0,
                            })),
                        message_type: message_type_enum,
                        business_type: first_task.metadata.get("business_type").cloned().unwrap_or_default(),
                        session_type: first_task.metadata.get("session_type").cloned().unwrap_or_default(),
                        status: String::new(),
                        extra: {
                            let mut extra = first_task.metadata.clone();
                            extra.insert("message_type".to_string(), first_task.message_type.clone());
                            extra
                        },
                        is_recalled: false,
                        recalled_at: None,
                        is_burn_after_read: false,
                        burn_after_seconds: 0,
                        tenant: tenant_context.clone(),
                        attachments: vec![],
                        audit: None,
                        tags: vec![],
                        timeline: None,
                        attributes: HashMap::new(),
                        read_by: vec![],
                        visibility: HashMap::new(),
                        operations: vec![],
                    }
                };
                
                let request = AccessPushRequest {
                    context: Some(flare_proto::RequestContext::default()),
                    tenant: tenant_context,
                    target_user_ids: user_ids.clone(),
                    message: Some(storage_message),
                    options: Some(AccessPushOptions {
                        require_ack: true, // 要求ACK确认
                        priority: 0,
                        expire_at_ms: 0,
                        offline_push: false,
                        device_ids: vec![],
                        platforms: vec![],
                    }),
                    metadata: HashMap::new(),
                };
                
                // 更新消息状态为推送中
                for (i, user_id) in user_ids.iter().enumerate() {
                    if i < message_ids.len() {
                        state_tracker
                            .update_status(
                                &message_ids[i],
                                user_id,
                                MessageStatus::Pushing,
                                None,
                            )
                            .await;
                        
                        // 注册待确认的ACK
                        ack_tracker
                            .register_pending_ack(&message_ids[i], user_id, AckType::PushAck)
                            .await;
                    }
                }
                
                // 带重试的推送
                let gateway_id_clone = gateway_id.clone();
                #[cfg(feature = "tracing")]
                let push_span = create_span("push-server", "online_push");
                #[cfg(feature = "tracing")]
                {
                    set_gateway_id(&push_span, &gateway_id_clone);
                    set_push_type(&push_span, "online");
                }
                
                push_tasks.push((gateway_id_clone.clone(), tokio::spawn(async move {
                    let gateway_id_for_retry = gateway_id_clone.clone();
                    let result = execute_with_retry(&retry_policy, move || {
                        let router = Arc::clone(&router);
                        let req = request.clone();
                        let gateway_id_inner = gateway_id_for_retry.clone();
                        async move { router.route_push_message(&gateway_id_inner, req).await }
                    })
                    .await
                    .map_err(|e| anyhow::anyhow!("Push failed after retries: {}", e));
                    
                    #[cfg(feature = "tracing")]
                    {
                        if result.is_err() {
                            if let Err(ref e) = result {
                                set_error(&push_span, &e.to_string());
                            }
                        }
                        push_span.end();
                    }
                    
                    result
                })));
            }
        }
        
        // 等待所有推送完成，并处理结果
        for (gateway_id, task) in push_tasks {
            let result = task.await;
            
            // 获取该gateway对应的tasks
            let gateway_tasks = gateway_tasks_map.get(&gateway_id)
                .cloned()
                .unwrap_or_default();
            
            match result {
                Ok(Ok(_)) => {
                    // 推送成功，更新所有相关任务的状态为已推送
                    for task in &gateway_tasks {
                        self.state_tracker
                            .update_status(
                                &task.message_id,
                                &task.user_id,
                                MessageStatus::Pushed,
                                None,
                            )
                            .await;
                        
                        // 记录在线推送成功
                        self.metrics.online_push_success_total
                            .with_label_values(&[&gateway_id, &tenant_id])
                            .inc();
                        
                        // 记录推送延迟
                        self.metrics.push_latency_seconds
                            .with_label_values(&["online", &tenant_id])
                            .observe(batch_start.elapsed().as_secs_f64());
                    }
                }
                Ok(Err(e)) => {
                    warn!(
                        error = %e,
                        gateway_id = %gateway_id,
                        user_count = gateway_tasks.len(),
                        "Push to gateway failed"
                    );
                    warn!(
                        gateway_id = %gateway_id,
                        ?e,
                        "Failed to push message to gateway after retries"
                    );
                    // 推送失败，降级到离线推送（仅普通消息）
                    for task in &gateway_tasks {
                        // 记录在线推送失败
                        self.metrics.online_push_failure_total
                            .with_label_values(&[&gateway_id, &e.to_string(), &tenant_id])
                            .inc();
                        if task.message_type != "Notification" && task.message_type != "notification" {
                            // 普通消息降级到离线推送
                            self.state_tracker
                                .update_status(
                                    &task.message_id,
                                    &task.user_id,
                                    MessageStatus::Failed,
                                    Some(e.to_string()),
                                )
                                .await;
                            // 添加到离线任务列表
                            offline_tasks.push(task.clone());
                        } else {
                            // 通知消息失败，直接标记为失败
                            self.state_tracker
                                .update_status(
                                    &task.message_id,
                                    &task.user_id,
                                    MessageStatus::Failed,
                                    Some(e.to_string()),
                                )
                                .await;
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        gateway_id = %gateway_id,
                        ?e,
                        "Task join error"
                    );
                    // Join错误，也降级到离线推送（仅普通消息）
                    for task in &gateway_tasks {
                        if task.message_type != "Notification" && task.message_type != "notification" {
                            offline_tasks.push(task.clone());
                        }
                    }
                }
            }
        }
        
        // 5. 处理离线用户
        self.handle_offline_tasks(offline_tasks).await?;
        
        Ok(())
    }
    
    /// 处理离线用户任务
    #[instrument(skip(self), fields(task_count))]
    async fn handle_offline_tasks(&self, tasks: Vec<PushDispatchTask>) -> Result<()> {
        let span = Span::current();
        span.record("task_count", tasks.len() as u64);
        
        let mut offline_normal = Vec::new();
        let mut offline_notification = Vec::new();
        
        // 按消息类型分类
        for task in tasks {
            let message_type = task.message_type.as_str();
            
            // 更新消息状态
            if message_type == "Notification" || message_type == "notification" {
                // 通知消息：离线舍弃
                self.state_tracker
                    .update_status(
                        &task.message_id,
                        &task.user_id,
                        MessageStatus::Expired,
                        None,
                    )
                    .await;
                offline_notification.push(task);
            } else {
                // 普通消息：进入离线推送队列
                self.state_tracker
                    .update_status(
                        &task.message_id,
                        &task.user_id,
                        MessageStatus::Pending,
                        None,
                    )
                    .await;
                offline_normal.push(task);
            }
        }
        
        // 批量发布离线推送任务（仅普通消息）
        if !offline_normal.is_empty() {
            #[cfg(feature = "tracing")]
            let offline_span = create_span("push-server", "offline_processing");
            
            // 提取租户ID和消息类型用于指标
            let tenant_id = offline_normal.first()
                .and_then(|t| t.tenant_id.as_deref())
                .unwrap_or("unknown");
            let message_type = offline_normal.first()
                .map(|t| t.message_type.as_str())
                .unwrap_or("unknown");
            
            #[cfg(feature = "tracing")]
            {
                set_push_type(&offline_span, "offline");
                offline_span.record("task_count", offline_normal.len() as u64);
            }
            
            match self.task_publisher
                .publish_offline_batch(&offline_normal)
                .await {
                Ok(_) => {
                    #[cfg(feature = "tracing")]
                    offline_span.end();
                }
                Err(e) => {
                    #[cfg(feature = "tracing")]
                    {
                        set_error(&offline_span, &e.to_string());
                        offline_span.end();
                    }
                    return Err(e);
                }
            }
            
            // 记录离线推送任务创建
            self.metrics.offline_push_tasks_created_total
                .with_label_values(&[message_type, tenant_id])
                .inc();
        }
        
        // 通知消息离线舍弃（记录日志）
        if !offline_notification.is_empty() {
            info!(
                count = offline_notification.len(),
                "Discarded offline notification messages"
            );
        }
        
        Ok(())
    }

    pub async fn handle_push_message(&self, request: PushMessageRequest) -> Result<()> {
        let tenant_ctx = request.tenant.clone();
        let tenant_ref = request.tenant.as_ref();
        let context_ref = request.context.as_ref();
        let metadata = context_ref.map(request_metadata_from_context);

        // 检查是否是聊天室消息（receiver_ids 为空）
        let is_chatroom_message = request.user_ids.is_empty();
        
        // 如果是聊天室消息，需要查询所有在线用户
        let user_ids_to_process: Vec<String> = if is_chatroom_message {
            // 解析消息以获取 session_type 和 chatroom 标记
            let mut is_chatroom = false;
            if let Ok(storage_message) = flare_proto::storage::Message::decode(request.message.as_slice()) {
                is_chatroom = storage_message.session_type == "group"
                    || storage_message.extra.get("chatroom")
                        .map(|v| v == "true")
                        .unwrap_or(false);
            }
            
            if is_chatroom {
                // 聊天室消息：查询所有在线用户
                let tenant_id = request.tenant.as_ref().map(|t| t.tenant_id.as_str());
                info!("Chatroom message detected, querying all online users");
                match self.online_repo.get_all_online_users(tenant_id).await {
                    Ok(online_statuses) => {
                        let user_ids: Vec<String> = online_statuses
                            .values()
                            .filter(|status| status.online)
                            .map(|status| status.user_id.clone())
                            .collect();
                        info!(
                            count = user_ids.len(),
                            tenant_id = tenant_id.unwrap_or("unknown"),
                            "Found {} online users for chatroom broadcast",
                            user_ids.len()
                        );
                        user_ids
                    }
                    Err(err) => {
                        warn!(
                            ?err,
                            tenant_id = tenant_id.unwrap_or("unknown"),
                            "Failed to query all online users for chatroom message"
                        );
                        vec![]
                    }
                }
            } else {
                // 不是聊天室消息，但 user_ids 为空，可能是配置错误
                warn!("PushMessageRequest has empty user_ids but is not a chatroom message");
                vec![]
            }
        } else {
            // 普通消息：使用 request.user_ids
            request.user_ids.clone()
        };

        // 构建 PushDispatchTask 列表，然后调用批量处理逻辑
        let mut tasks = Vec::new();
        
        for user_id in &user_ids_to_process {
            let online = match self.online_repo.is_online(user_id).await {
                Ok(online) => online,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to load online state");
                    false
                }
            };

            if !online && !should_persist_offline(&request.options) {
                info!(user_id = %user_id, "skip offline user due to persist_if_offline = false");
                continue;
            }

            let (envelope, mut draft) = prepare_message_envelope(
                &self.config.default_tenant_id,
                tenant_ref,
                user_id,
                context_ref,
                request.options.as_ref(),
                request.message.clone(),
            );

            if let Err(err) = self
                .hooks
                .pre_send(envelope.hook_context(), &mut draft)
                .await
            {
                warn!(user_id = %user_id, ?err, "pre-dispatch hook rejected push message");
                continue;
            }

            let message_id = envelope.message_id(&draft);
            let message_type_label = draft
                .metadata
                .get("message_type")
                .cloned()
                .unwrap_or_else(|| envelope.message_type().to_string());
            let task = self.build_task(
                user_id.clone(),
                message_id.clone(),
                message_type_label,
                draft.payload.clone(),
                None,
                draft.headers.clone(),
                draft.metadata.clone(),
                online,
                request.options.as_ref(),
                tenant_identifier(tenant_ctx.as_ref(), &self.config.default_tenant_id),
                metadata.clone(),
            );

            tasks.push(task);
        }
        
        // 使用批量处理逻辑推送到 Access Gateway
        if !tasks.is_empty() {
            info!(
                task_count = tasks.len(),
                "Processing {} tasks from handle_push_message",
                tasks.len()
            );
            self.process_push_tasks_batch(tasks).await?;
        }

        Ok(())
    }

    pub async fn handle_push_notification(&self, request: PushNotificationRequest) -> Result<()> {
        let tenant_ctx = request.tenant.clone();
        let tenant_ref = request.tenant.as_ref();
        let context_ref = request.context.as_ref();
        let metadata = context_ref.map(request_metadata_from_context);

        let base_notification = match request.notification.clone() {
            Some(notification) => DispatchNotification::from(notification),
            None => {
                return Err(ErrorBuilder::new(
                    ErrorCode::InvalidParameter,
                    "notification payload is required",
                )
                .build_error());
            }
        };

        for user_id in &request.user_ids {
            let online = match self.online_repo.is_online(user_id).await {
                Ok(online) => online,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to load online state");
                    false
                }
            };

            if !online && !should_persist_offline(&request.options) {
                info!(user_id = %user_id, "skip offline notification due to persist_if_offline = false");
                continue;
            }

            let preparation = prepare_notification_envelope(
                &self.config.default_tenant_id,
                tenant_ref,
                user_id,
                context_ref,
                request.options.as_ref(),
                &base_notification,
            );

            let (envelope, mut draft) = match preparation {
                Ok(result) => result,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to prepare notification hook payload");
                    continue;
                }
            };

            if let Err(err) = self
                .hooks
                .pre_send(envelope.hook_context(), &mut draft)
                .await
            {
                warn!(user_id = %user_id, ?err, "pre-dispatch hook rejected notification");
                continue;
            }

            let final_notification = match finalize_notification(&draft) {
                Ok(notification) => notification,
                Err(err) => {
                    warn!(user_id = %user_id, ?err, "failed to decode notification after hook");
                    continue;
                }
            };

            let message_id = envelope.message_id(&draft);
            let task = self.build_task(
                user_id.clone(),
                message_id.clone(),
                envelope.message_type().to_string(),
                Vec::new(),
                Some(final_notification),
                draft.headers.clone(),
                draft.metadata.clone(),
                online,
                request.options.as_ref(),
                tenant_identifier(tenant_ctx.as_ref(), &self.config.default_tenant_id),
                metadata.clone(),
            );

            self.task_publisher.publish(&task).await?;

            if let Err(err) = self
                .hooks
                .post_send(
                    envelope.hook_context(),
                    &build_post_send_record(&envelope, &draft),
                    &draft,
                )
                .await
            {
                warn!(user_id = %user_id, ?err, "post-dispatch hook failed for notification");
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_task(
        &self,
        user_id: String,
        message_id: String,
        message_type: String,
        message: Vec<u8>,
        notification: Option<DispatchNotification>,
        headers: HashMap<String, String>,
        metadata: HashMap<String, String>,
        online: bool,
        options: Option<&PushOptions>,
        tenant_id: Option<String>,
        context: Option<RequestMetadata>,
    ) -> PushDispatchTask {
        let (require_online, persist_if_offline, priority) = options
            .map(|opt| (opt.require_online, opt.persist_if_offline, opt.priority))
            .unwrap_or((false, true, 0));

        PushDispatchTask {
            user_id,
            message_id,
            message_type,
            message,
            notification,
            headers,
            metadata,
            online,
            tenant_id,
            require_online,
            persist_if_offline,
            priority,
            context,
        }
    }
}

impl From<flare_proto::push::Notification> for DispatchNotification {
    fn from(value: flare_proto::push::Notification) -> Self {
        Self {
            title: value.title,
            body: value.body,
            data: value.data,
            metadata: value.metadata,
        }
    }
}

fn tenant_identifier(
    tenant: Option<&flare_proto::common::TenantContext>,
    default: &str,
) -> Option<String> {
    tenant
        .and_then(|tenant| {
            if tenant.tenant_id.is_empty() {
                None
            } else {
                Some(tenant.tenant_id.clone())
            }
        })
        .or_else(|| Some(default.to_string()))
}

fn request_metadata_from_context(context: &flare_proto::common::RequestContext) -> RequestMetadata {
    fn to_opt(value: &str) -> Option<String> {
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    let trace_id = context.trace.as_ref()
        .filter(|t| !t.trace_id.is_empty())
        .map(|t| t.trace_id.clone());
    
    let span_id = context.trace.as_ref()
        .filter(|t| !t.span_id.is_empty())
        .map(|t| t.span_id.clone());
    
    let client_ip = context.device.as_ref()
        .filter(|d| !d.ip_address.is_empty())
        .map(|d| d.ip_address.clone());

    RequestMetadata {
        request_id: context.request_id.clone(),
        trace_id,
        span_id,
        client_ip,
        user_agent: to_opt(&context.user_agent),
    }
}

fn should_persist_offline(options: &Option<PushOptions>) -> bool {
    options
        .as_ref()
        .map(|opt| opt.persist_if_offline)
        .unwrap_or(true)
}

