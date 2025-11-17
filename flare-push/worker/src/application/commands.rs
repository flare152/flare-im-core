//! 应用层命令模块

use std::sync::Arc;
use std::time::Instant;

use flare_im_core::metrics::PushWorkerMetrics;
#[cfg(feature = "tracing")]
use flare_im_core::tracing::{create_span, set_user_id, set_platform, set_tenant_id, set_push_type, set_retry_count, set_error};
use flare_server_core::error::Result;
use tracing::{error, info, warn, instrument, Span};

use crate::config::PushWorkerConfig;
use crate::domain::models::PushDispatchTask;
use crate::domain::repositories::{OfflinePushSender, OnlinePushSender};
use crate::infrastructure::ack_publisher::{AckPublisher, PushAckEvent};
use crate::infrastructure::dlq_publisher::DlqPublisher;
use crate::infrastructure::retry::{execute_with_retry, RetryPolicy};

pub struct PushExecutionCommandService {
    config: Arc<PushWorkerConfig>,
    online_sender: Arc<dyn OnlinePushSender>,
    offline_sender: Arc<dyn OfflinePushSender>,
    ack_publisher: Arc<dyn AckPublisher>,
    dlq_publisher: Arc<dyn DlqPublisher>,
    retry_policy: RetryPolicy,
    metrics: Arc<PushWorkerMetrics>,
}

impl PushExecutionCommandService {
    pub fn new(
        config: Arc<PushWorkerConfig>,
        online_sender: Arc<dyn OnlinePushSender>,
        offline_sender: Arc<dyn OfflinePushSender>,
        ack_publisher: Arc<dyn AckPublisher>,
        dlq_publisher: Arc<dyn DlqPublisher>,
        metrics: Arc<PushWorkerMetrics>,
    ) -> Self {
        let retry_policy = RetryPolicy::from_config(
            config.push_retry_max_attempts,
            config.push_retry_initial_delay_ms,
            config.push_retry_max_delay_ms,
            config.push_retry_backoff_multiplier,
        );

        Self {
            config,
            online_sender,
            offline_sender,
            ack_publisher,
            dlq_publisher,
            retry_policy,
            metrics,
        }
    }

    #[instrument(skip(self), fields(user_id, tenant_id, platform))]
    pub async fn execute(&self, task: PushDispatchTask) -> Result<()> {
        let start = Instant::now();
        let span = Span::current();
        
        // 提取租户ID和平台用于指标
        let tenant_id = task.tenant_id.as_deref().unwrap_or("unknown");
        let platform = task.metadata.get("platform").map(|s| s.as_str()).unwrap_or("unknown");
        
        // 设置追踪属性
        #[cfg(feature = "tracing")]
        {
            set_tenant_id(&span, tenant_id);
            set_platform(&span, platform);
            set_user_id(&span, &task.user_id);
            set_push_type(&span, if task.online { "online" } else { "offline" });
            span.record("user_id", &task.user_id);
        }
        
        if task.require_online && !task.online {
            info!(user_id = %task.user_id, "skip offline task due to require_online=true");
            return Ok(());
        }

        // 执行推送（带重试）
        let result = if task.online {
            self.execute_with_retry(|| self.online_sender.send(&task)).await
        } else if task.persist_if_offline {
            self.execute_with_retry(|| self.offline_sender.send(&task)).await
        } else {
            warn!(user_id = %task.user_id, "offline task dropped because persist_if_offline=false");
            return Ok(());
        };
        
        // 记录推送耗时
        let duration = start.elapsed();
        self.metrics.push_duration_seconds
            .with_label_values(&[platform, tenant_id])
            .observe(duration.as_secs_f64());

        // 处理推送结果
        match result {
            Ok(_) => {
                // 推送成功，上报ACK
                self.publish_ack(&task, true, None).await?;
                
                // 记录离线推送成功（仅离线推送）
                if !task.online {
                    self.metrics.offline_push_success_total
                        .with_label_values(&[platform, tenant_id])
                        .inc();
                }
                
                Ok(())
            }
            Err(e) => {
                // 推送失败，上报ACK并发送到死信队列
                let error_str = e.to_string();
                error!(
                    message_id = %task.message_id,
                    user_id = %task.user_id,
                    error = %error_str,
                    "Push failed after retries"
                );
                
                // 记录离线推送失败（仅离线推送）
                if !task.online {
                    self.metrics.offline_push_failure_total
                        .with_label_values(&[platform, &error_str, tenant_id])
                        .inc();
                }
                
                // 上报失败ACK
                let _ = self.publish_ack(&task, false, Some(&error_str)).await;
                
                // 发送到死信队列
                self.dlq_publisher.publish_to_dlq(&task, &error_str).await?;
                
                // 记录死信队列消息数
                self.metrics.dlq_messages_total
                    .with_label_values(&[&error_str, &tenant_id.to_string()])
                    .inc();
                
                Ok(()) // 返回Ok，避免重复处理
            }
        }
    }

    /// 带重试的执行推送
    async fn execute_with_retry<F, Fut>(&self, mut f: F) -> std::result::Result<(), String>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<()>>,
    {
        let mut attempt = 0;
        let mut last_error = None;
        
        while attempt < self.retry_policy.max_attempts {
            match f().await {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let error_str = e.to_string();
                    if attempt < self.retry_policy.max_attempts - 1 {
                        // 可重试的错误，等待后重试
                        let delay = self.retry_policy.calculate_delay(attempt);
                        tokio::time::sleep(delay).await;
                        last_error = Some(error_str);
                        attempt += 1;
                        continue;
                    } else {
                        // 达到最大重试次数
                        return Err(error_str);
                    }
                }
            }
        }
        
        Err(last_error.unwrap_or_else(|| "Max retries exceeded".to_string()))
    }

    /// 上报ACK
    async fn publish_ack(
        &self,
        task: &PushDispatchTask,
        success: bool,
        error: Option<&str>,
    ) -> Result<()> {
        let event = PushAckEvent {
            message_id: task.message_id.clone(),
            user_id: task.user_id.clone(),
            success,
            error: error.map(|s| s.to_string()),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64,
        };

        self.ack_publisher.publish_ack(&event).await
    }
}

