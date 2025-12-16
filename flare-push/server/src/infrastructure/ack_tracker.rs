//! ACK确认机制（使用统一的 AckManager）
//!
//! 重构为使用 flare-im-core 的 AckManager，统一 ACK 管理

use std::sync::Arc;
use std::time::Duration;

use deadpool_redis::Pool;
use tracing::{debug, error, info, instrument, warn};

use crate::config::PushServerConfig;
use crate::domain::repository::PushTaskPublisher;
use flare_im_core::ack::{AckModule, AckStatus, AckType, ImportanceLevel};
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};

/// ACK跟踪器（使用统一的 AckManager）
pub struct AckTracker {
    /// 统一的 ACK 管理器
    ack_manager: Arc<AckModule>,
    /// 配置
    config: Arc<PushServerConfig>,
    /// 推送任务发布器（用于重新推送）
    task_publisher: Option<Arc<dyn PushTaskPublisher>>,
    /// Redis连接池（用于持久化重试计数）
    redis_pool: Option<Pool>,
}

impl AckTracker {
    /// 创建新的 ACK 跟踪器
    pub fn new(ack_manager: Arc<AckModule>, config: Arc<PushServerConfig>) -> Arc<Self> {
        Arc::new(Self {
            ack_manager,
            config,
            task_publisher: None,
            redis_pool: None,
        })
    }

    pub fn with_task_publisher(
        mut self: Arc<Self>,
        task_publisher: Arc<dyn PushTaskPublisher>,
    ) -> Arc<Self> {
        Arc::get_mut(&mut self).unwrap().task_publisher = Some(task_publisher);
        self
    }

    pub fn with_redis_pool(mut self: Arc<Self>, redis_pool: Pool) -> Arc<Self> {
        Arc::get_mut(&mut self).unwrap().redis_pool = Some(redis_pool);
        self
    }

    /// 注册待确认的ACK
    pub async fn register_pending_ack(&self, message_id: &str, user_id: &str) -> Result<()> {
        let ack_info = flare_im_core::ack::AckStatusInfo {
            message_id: message_id.to_string(),
            user_id: user_id.to_string(),
            ack_type: Some(AckType::ServerAck), // 推送 ACK（使用 ServerAck）
            status: AckStatus::Pending,
            timestamp: chrono::Utc::now().timestamp() as u64,
            importance: ImportanceLevel::High, // 推送 ACK 高优先级
        };

        self.ack_manager
            .record_ack_status(ack_info)
            .await
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to register pending ACK: {}", e),
                )
                .build_error()
            })?;

        debug!(
            message_id = %message_id,
            user_id = %user_id,
            "Registered pending ACK"
        );

        Ok(())
    }

    /// 确认ACK
    pub async fn confirm_ack(&self, message_id: &str, user_id: &str) -> Result<bool> {
        let start_time = std::time::Instant::now();
        debug!(
            message_id = %message_id,
            user_id = %user_id,
            "Checking ACK status"
        );

        // 查询 ACK 状态
        if let Some(ack_info) = self
            .ack_manager
            .service
            .get_ack_status(message_id, user_id)
            .await
            .map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to get ACK status: {}", e),
                )
                .build_error()
            })?
        {
            if ack_info.status == AckStatus::Pending {
                // 更新为已确认
                let updated_ack_info = flare_im_core::ack::AckStatusInfo {
                    message_id: message_id.to_string(),
                    user_id: user_id.to_string(),
                    ack_type: ack_info.ack_type,
                    status: AckStatus::Received,
                    timestamp: chrono::Utc::now().timestamp() as u64,
                    importance: ImportanceLevel::High,
                };

                self.ack_manager
                    .record_ack_status(updated_ack_info)
                    .await
                    .map_err(|e| {
                        ErrorBuilder::new(
                            ErrorCode::InternalError,
                            format!("Failed to confirm ACK: {}", e),
                        )
                        .build_error()
                    })?;

                let duration_ms = start_time.elapsed().as_millis();
                info!(
                    message_id = %message_id,
                    user_id = %user_id,
                    duration_ms = duration_ms,
                    "ACK confirmed, retry stopped"
                );
                return Ok(true);
            } else {
                let duration_ms = start_time.elapsed().as_millis();
                debug!(
                    message_id = %message_id,
                    user_id = %user_id,
                    status = ?ack_info.status,
                    duration_ms = duration_ms,
                    "ACK already confirmed or not pending"
                );
            }
        } else {
            let duration_ms = start_time.elapsed().as_millis();
            debug!(
                message_id = %message_id,
                user_id = %user_id,
                duration_ms = duration_ms,
                "ACK not found in tracker"
            );
        }

        Ok(false)
    }

    /// 启动ACK监控任务（使用轮询方式检查超时）
    ///
    /// ACK 传递机制已完善（Gateway → Push Proxy → Kafka → Push Server），现在启用超时监控
    pub fn start_monitor(self: Arc<Self>) -> tokio::task::JoinHandle<()> {
        let task_publisher = self.task_publisher.clone();
        let redis_pool = self.redis_pool.clone();
        let config = self.config.clone();
        let ack_manager = self.ack_manager.clone();

        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(config.ack_monitor_interval_seconds));

            loop {
                interval.tick().await;

                // 轮询检查超时的 ACK
                // 从 Redis 中查询所有 Pending 状态的 ACK，检查是否超时
                // 注意：deadpool_redis::Pool 实现了 Clone，可以直接克隆
                // 将 Pool 包装在 Arc 中，以便在 poll_timeout_acks 和 spawn 中使用
                let redis_pool_arc = redis_pool.as_ref().map(|p| Arc::new(p.clone()));
                if let Err(e) = Self::poll_timeout_acks(
                    &ack_manager,
                    &config,
                    task_publisher.clone(),
                    redis_pool_arc.clone(),
                )
                .await
                {
                    error!(error = %e, "Failed to poll timeout ACKs");
                }
            }
        })
    }

    /// 轮询检查超时的 ACK
    ///
    /// 从 Redis 中扫描所有 ACK keys，检查 Pending 状态的 ACK 是否超时
    /// 使用 SCAN 命令避免阻塞 Redis，支持大量 ACK 的查询
    ///
    /// # 性能优化
    /// - 分批扫描：限制单次扫描的 keys 数量
    /// - 分批 Pipeline：避免单个 Pipeline 太大
    /// - 批量处理：批量处理超时事件，限制并发数量
    async fn poll_timeout_acks(
        ack_manager: &Arc<AckModule>,
        config: &PushServerConfig,
        task_publisher: Option<Arc<dyn PushTaskPublisher>>,
        redis_pool: Option<Arc<Pool>>,
    ) -> Result<()> {
        let start_time = std::time::Instant::now();
        let timeout_seconds = config.ack_timeout_seconds;
        let current_timestamp = chrono::Utc::now().timestamp() as u64;
        let timeout_threshold = current_timestamp.saturating_sub(timeout_seconds as u64);

        // 性能优化配置
        let scan_batch_size = config.ack_scan_batch_size;
        let pipeline_batch_size = config.ack_pipeline_batch_size;
        let timeout_batch_size = config.ack_timeout_batch_size;
        let concurrent_limit = config.ack_timeout_concurrent_limit;

        debug!(
            timeout_seconds = timeout_seconds,
            current_timestamp = current_timestamp,
            timeout_threshold = timeout_threshold,
            scan_batch_size = scan_batch_size,
            pipeline_batch_size = pipeline_batch_size,
            "Starting ACK timeout check (optimized)"
        );

        // 1. 从 Redis 扫描所有 ACK keys（分批扫描，限制数量）
        let redis_manager = &ack_manager.service.redis_manager;
        let ack_keys = match redis_manager
            .scan_all_ack_keys(
                Some(scan_batch_size * 10), // 最多扫描 10 个批次
                scan_batch_size,
            )
            .await
        {
            Ok(keys) => {
                debug!(
                    key_count = keys.len(),
                    "Scanned {} ACK keys from Redis (limited)",
                    keys.len()
                );
                keys
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to scan ACK keys from Redis"
                );
                return Err(ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to scan ACK keys: {}", e),
                )
                .build_error());
            }
        };

        if ack_keys.is_empty() {
            debug!("No ACK keys found in Redis");
            return Ok(());
        }

        // 2. 批量获取 ACK 状态信息（分批 Pipeline）
        let ack_infos = match redis_manager
            .batch_get_ack_status_from_keys(&ack_keys, pipeline_batch_size)
            .await
        {
            Ok(infos) => {
                debug!(
                    ack_count = infos.len(),
                    "Retrieved {} ACK statuses from Redis (batched pipeline)",
                    infos.len()
                );
                infos
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to batch get ACK statuses from Redis"
                );
                return Err(ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to batch get ACK statuses: {}", e),
                )
                .build_error());
            }
        };

        // 3. 过滤出 Pending 状态且超时的 ACK
        let mut timeout_events = Vec::new();
        let mut processed_count = 0;

        for ack_info in &ack_infos {
            // 只检查 Pending 状态的 ACK
            if ack_info.status != AckStatus::Pending {
                continue;
            }

            // 检查是否超时
            if ack_info.timestamp <= timeout_threshold {
                let ack_type = ack_info.ack_type.unwrap_or(AckType::ServerAck);
                let timeout_event = flare_im_core::ack::AckTimeoutEvent {
                    message_id: ack_info.message_id.clone(),
                    user_id: ack_info.user_id.clone(),
                    ack_type,
                    timeout_at: current_timestamp as i64,
                };
                timeout_events.push(timeout_event);

                debug!(
                    message_id = %ack_info.message_id,
                    user_id = %ack_info.user_id,
                    timestamp = ack_info.timestamp,
                    elapsed_seconds = current_timestamp.saturating_sub(ack_info.timestamp),
                    "ACK timeout detected"
                );
            }

            processed_count += 1;
        }

        // 4. 批量处理超时事件（限制并发数量）
        let timeout_count = timeout_events.len();
        if timeout_count > 0 {
            info!(
                timeout_count = timeout_count,
                "Processing {} timeout ACKs in batches", timeout_count
            );

            // 使用信号量限制并发数量
            let semaphore = Arc::new(tokio::sync::Semaphore::new(concurrent_limit));
            let mut handles = Vec::new();

            // 分批处理超时事件
            for chunk in timeout_events.chunks(timeout_batch_size) {
                let chunk_events = chunk.to_vec();
                let task_publisher_clone = task_publisher.clone();
                let redis_pool_clone = redis_pool.clone();
                let config_clone = config.clone();
                let semaphore_clone = semaphore.clone();

                // 为每个批次创建一个任务
                let handle = tokio::spawn(async move {
                    // 获取信号量许可（限制并发）
                    let permit = semaphore_clone.acquire().await;
                    if permit.is_err() {
                        error!("Failed to acquire semaphore permit for timeout ACK batch");
                        return;
                    }
                    let _permit = permit.unwrap();

                    // 批量处理该批次的所有超时事件
                    for timeout_event in chunk_events {
                        Self::handle_ack_timeout(
                            &timeout_event,
                            task_publisher_clone.as_deref(),
                            redis_pool_clone.as_ref().map(|arc_pool| &**arc_pool),
                            &config_clone,
                        )
                        .await;
                    }
                });

                handles.push(handle);
            }

            // 等待所有批次完成（可选：可以异步等待，不阻塞主循环）
            // 这里选择异步等待，不阻塞主循环
            tokio::spawn(async move {
                for handle in handles {
                    if let Err(e) = handle.await {
                        error!(error = %e, "Failed to process timeout ACK batch");
                    }
                }
            });
        }

        let duration_ms = start_time.elapsed().as_millis();
        info!(
            total_keys = ack_keys.len(),
            total_acks = ack_infos.len(),
            pending_acks = processed_count,
            timeout_acks = timeout_count,
            duration_ms = duration_ms,
            scan_batch_size = scan_batch_size,
            pipeline_batch_size = pipeline_batch_size,
            timeout_batch_size = timeout_batch_size,
            concurrent_limit = concurrent_limit,
            "ACK timeout check completed (optimized)"
        );

        Ok(())
    }

    /// 处理ACK超时
    #[instrument(skip(task_publisher, redis_pool, config), fields(message_id = %timeout_event.message_id, user_id = %timeout_event.user_id))]
    async fn handle_ack_timeout(
        timeout_event: &flare_im_core::ack::AckTimeoutEvent,
        task_publisher: Option<&dyn PushTaskPublisher>,
        redis_pool: Option<&Pool>,
        config: &PushServerConfig,
    ) {
        // 检查重试次数
        let retry_count = Self::get_retry_count(
            &timeout_event.message_id,
            &timeout_event.user_id,
            redis_pool,
        )
        .await
        .unwrap_or(0);

        let max_retries = config.ack_timeout_max_retries;

        if retry_count < max_retries {
            // 未超过最大重试次数，触发重试
            info!(
                message_id = %timeout_event.message_id,
                user_id = %timeout_event.user_id,
                retry_count = retry_count,
                max_retries = max_retries,
                "Retrying push due to ACK timeout"
            );

            // 增加重试计数
            if let Err(e) = Self::increment_retry_count(
                &timeout_event.message_id,
                &timeout_event.user_id,
                redis_pool,
            )
            .await
            {
                error!(error = %e, "Failed to increment retry count");
            }

            // 重新推送消息
            if let Err(e) = Self::retry_push(
                &timeout_event.message_id,
                &timeout_event.user_id,
                task_publisher,
                redis_pool,
            )
            .await
            {
                error!(error = %e, "Failed to retry push");
            }
        } else {
            // 超过最大重试次数，降级到离线推送
            warn!(
                message_id = %timeout_event.message_id,
                user_id = %timeout_event.user_id,
                retry_count = retry_count,
                max_retries = max_retries,
                "Max retries exceeded, falling back to offline push"
            );

            // 清理重试计数
            if let Err(e) = Self::clear_retry_count(
                &timeout_event.message_id,
                &timeout_event.user_id,
                redis_pool,
            )
            .await
            {
                error!(error = %e, "Failed to clear retry count");
            }

            // 降级到离线推送
            if let Err(e) = Self::fallback_to_offline_push(
                &timeout_event.message_id,
                &timeout_event.user_id,
                timeout_event.ack_type,
                task_publisher,
                redis_pool,
            )
            .await
            {
                error!(error = %e, "Failed to fallback to offline push");
            }
        }
    }

    /// 获取重试次数
    async fn get_retry_count(
        message_id: &str,
        user_id: &str,
        redis_pool: Option<&Pool>,
    ) -> Result<u32> {
        let retry_key = format!("ack_retry:{}:{}", message_id, user_id);

        if let Some(redis_pool) = redis_pool {
            let mut conn = redis_pool.get().await.map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to get Redis connection: {}", e),
                )
                .build_error()
            })?;

            let count: Option<String> = redis::cmd("GET")
                .arg(&retry_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::InternalError,
                        format!("Failed to get retry count: {}", e),
                    )
                    .build_error()
                })?;

            Ok(count.and_then(|s| s.parse().ok()).unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    /// 增加重试计数
    async fn increment_retry_count(
        message_id: &str,
        user_id: &str,
        redis_pool: Option<&Pool>,
    ) -> Result<()> {
        let retry_key = format!("ack_retry:{}:{}", message_id, user_id);

        if let Some(redis_pool) = redis_pool {
            let mut conn = redis_pool.get().await.map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to get Redis connection: {}", e),
                )
                .build_error()
            })?;

            let _: () = redis::cmd("INCR")
                .arg(&retry_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::InternalError,
                        format!("Failed to increment retry count: {}", e),
                    )
                    .build_error()
                })?;

            // 设置过期时间（24小时）
            let _: () = redis::cmd("EXPIRE")
                .arg(&retry_key)
                .arg(86400)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::InternalError,
                        format!("Failed to set retry key expiry: {}", e),
                    )
                    .build_error()
                })?;
        }

        Ok(())
    }

    /// 清理重试计数
    async fn clear_retry_count(
        message_id: &str,
        user_id: &str,
        redis_pool: Option<&Pool>,
    ) -> Result<()> {
        let retry_key = format!("ack_retry:{}:{}", message_id, user_id);

        if let Some(redis_pool) = redis_pool {
            let mut conn = redis_pool.get().await.map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to get Redis connection: {}", e),
                )
                .build_error()
            })?;

            let _: () = redis::cmd("DEL")
                .arg(&retry_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::InternalError,
                        format!("Failed to clear retry count: {}", e),
                    )
                    .build_error()
                })?;
        }

        Ok(())
    }

    /// 重新推送消息
    async fn retry_push(
        message_id: &str,
        user_id: &str,
        task_publisher: Option<&dyn PushTaskPublisher>,
        redis_pool: Option<&Pool>,
    ) -> Result<()> {
        info!(
            message_id = %message_id,
            user_id = %user_id,
            "Initiating push retry"
        );

        // 从Redis中获取原始推送任务
        if let Some(redis_pool) = redis_pool {
            let retry_key = format!("push_task:{}:{}", message_id, user_id);
            let mut conn = redis_pool.get().await.map_err(|e| {
                ErrorBuilder::new(
                    ErrorCode::InternalError,
                    format!("Failed to get Redis connection: {}", e),
                )
                .build_error()
            })?;

            let task_data: Option<Vec<u8>> = redis::cmd("GET")
                .arg(&retry_key)
                .query_async(&mut conn)
                .await
                .map_err(|e| {
                    ErrorBuilder::new(
                        ErrorCode::InternalError,
                        format!("Failed to get push task from Redis: {}", e),
                    )
                    .build_error()
                })?;

            if let Some(data) = task_data {
                match serde_json::from_slice::<crate::domain::model::PushDispatchTask>(&data) {
                    Ok(task) => {
                        if let Some(publisher) = task_publisher {
                            publisher.publish(&task).await?;
                            info!(
                                message_id = %message_id,
                                user_id = %user_id,
                                "Successfully resent push task"
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            message_id = %message_id,
                            user_id = %user_id,
                            error = %e,
                            "Failed to deserialize push task for retry"
                        );
                    }
                }
            }
        }

        Ok(())
    }

    /// 降级到离线推送
    async fn fallback_to_offline_push(
        message_id: &str,
        user_id: &str,
        ack_type: AckType,
        task_publisher: Option<&dyn PushTaskPublisher>,
        redis_pool: Option<&Pool>,
    ) -> Result<()> {
        match ack_type {
            AckType::StorageAck => {
                // 存储 ACK 失败，需要离线推送
                warn!(
                    message_id = %message_id,
                    user_id = %user_id,
                    "Storage ACK failed, falling back to offline push"
                );

                // 从Redis中获取原始推送任务
                if let Some(redis_pool) = redis_pool {
                    let retry_key = format!("push_task:{}:{}", message_id, user_id);
                    let mut conn = redis_pool.get().await.map_err(|e| {
                        ErrorBuilder::new(
                            ErrorCode::InternalError,
                            format!("Failed to get Redis connection: {}", e),
                        )
                        .build_error()
                    })?;

                    let task_data: Option<Vec<u8>> = redis::cmd("GET")
                        .arg(&retry_key)
                        .query_async(&mut conn)
                        .await
                        .map_err(|e| {
                            ErrorBuilder::new(
                                ErrorCode::InternalError,
                                format!("Failed to get push task from Redis: {}", e),
                            )
                            .build_error()
                        })?;

                    if let Some(data) = task_data {
                        match serde_json::from_slice::<crate::domain::model::PushDispatchTask>(
                            &data,
                        ) {
                            Ok(task) => {
                                if let Some(publisher) = task_publisher {
                                    publisher.publish(&task).await?;
                                    info!(
                                        message_id = %message_id,
                                        user_id = %user_id,
                                        "Successfully sent to offline push queue"
                                    );
                                }
                            }
                            Err(e) => {
                                error!(
                                    message_id = %message_id,
                                    user_id = %user_id,
                                    error = %e,
                                    "Failed to deserialize push task for offline push"
                                );
                            }
                        }
                    }
                }
            }
            _ => {
                // 其他类型无需离线推送
                info!(
                    message_id = %message_id,
                    user_id = %user_id,
                    "No offline push needed for this ACK type"
                );
            }
        }

        Ok(())
    }
}
