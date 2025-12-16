//! Kafka 消费者实现

use std::sync::Arc;
use std::time::Instant;

use crate::application::handlers::PushCommandHandler;
use crate::config::PushWorkerConfig;
use flare_im_core::metrics::PushWorkerMetrics;
use flare_server_core::error::{ErrorBuilder, ErrorCode, Result};
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::BorrowedMessage;
use rdkafka::{ClientConfig, Message};
use serde_json;
use tracing::{error, info};

use crate::domain::model::PushDispatchTask;

pub struct PushWorkerConsumer {
    config: Arc<PushWorkerConfig>,
    consumer: StreamConsumer,
    command_handler: Arc<PushCommandHandler>,
    metrics: Arc<PushWorkerMetrics>,
}

impl PushWorkerConsumer {
    pub async fn new(
        config: Arc<PushWorkerConfig>,
        command_handler: Arc<PushCommandHandler>,
        metrics: Arc<PushWorkerMetrics>,
    ) -> Result<Self> {
        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            "Creating Kafka consumer..."
        );

        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("group.id", &config.consumer_group)
            .set("auto.offset.reset", "earliest")
            .set("enable.partition.eof", "false")
            .set("session.timeout.ms", "30000")
            .set("enable.auto.commit", "false") // 手动提交，批量处理
            // 注意：max.poll.records 不是 rdkafka 的有效配置项，批量消费由代码逻辑控制
            .set("fetch.min.bytes", &config.fetch_min_bytes.to_string())
            .set("fetch.wait.max.ms", &config.fetch_max_wait_ms.to_string())
            // 网络和连接配置
            .set("security.protocol", "plaintext")
            .set("client.dns.lookup", "use_all_dns_ips")
            .set("broker.address.family", "v4")
            .set("metadata.max.age.ms", "300000") // 5 分钟
            // 消息大小限制
            .set("fetch.message.max.bytes", "10485760") // 10MB
            .set("max.partition.fetch.bytes", "10485760") // 10MB
            .create()
            .map_err(|e| {
                error!(
                    error = %e,
                    bootstrap = %config.kafka_bootstrap,
                    group = %config.consumer_group,
                    "Failed to create Kafka consumer"
                );
                ErrorBuilder::new(ErrorCode::ServiceUnavailable, "Failed to create consumer")
                    .details(e.to_string())
                    .build_error()
            })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            "Subscribing to Kafka topic..."
        );

        consumer.subscribe(&[&config.task_topic]).map_err(|e| {
            error!(
                error = %e,
                bootstrap = %config.kafka_bootstrap,
                group = %config.consumer_group,
                task_topic = %config.task_topic,
                "Failed to subscribe to Kafka topic"
            );
            ErrorBuilder::new(ErrorCode::ServiceUnavailable, "Failed to subscribe")
                .details(e.to_string())
                .build_error()
        })?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            group = %config.consumer_group,
            task_topic = %config.task_topic,
            "Successfully subscribed to Kafka topic"
        );

        Ok(Self {
            config,
            consumer,
            command_handler,
            metrics,
        })
    }

    pub fn config(&self) -> &PushWorkerConfig {
        &self.config
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting push worker consumer");

        loop {
            // 批量消费消息
            let mut batch = Vec::new();

            // 收集一批消息
            for _ in 0..self.config.max_poll_records {
                match tokio::time::timeout(
                    std::time::Duration::from_millis(self.config.fetch_max_wait_ms),
                    self.consumer.recv(),
                )
                .await
                {
                    Ok(Ok(message)) => {
                        batch.push(message);
                    }
                    Ok(Err(e)) => {
                        error!(?e, "Error receiving message");
                        return Err(ErrorBuilder::new(
                            ErrorCode::ServiceUnavailable,
                            "Consumer error",
                        )
                        .details(e.to_string())
                        .build_error());
                    }
                    Err(_) => {
                        // 超时，处理已收集的消息
                        break;
                    }
                }
            }

            // 批量处理消息
            if !batch.is_empty() {
                if let Err(e) = self.handle_batch(batch).await {
                    error!(?e, "Failed to process batch");
                }
            }
        }
    }

    async fn handle_batch(&self, messages: Vec<BorrowedMessage<'_>>) -> Result<()> {
        let batch_start = Instant::now();
        let batch_size = messages.len() as f64;

        // 记录批量大小
        self.metrics.batch_size.observe(batch_size);

        let mut tasks = Vec::new();

        // 解析所有消息
        for message in &messages {
            if let Ok(task) = self.parse_message(message) {
                tasks.push(task);
            }
        }

        // 批量处理任务
        if !tasks.is_empty() {
            let command = crate::application::commands::BatchExecutePushTasksCommand {
                tasks: tasks.clone(),
            };
            if let Err(e) = self
                .command_handler
                .handle_batch_execute_push_tasks(command)
                .await
            {
                error!(error = %e, "Failed to process batch tasks");
            }
        }

        // 记录批量处理耗时
        let _batch_duration = batch_start.elapsed();
        // 注意：PushWorkerMetrics 没有批量处理耗时指标，这里先不记录

        // 手动提交offset
        // 注意：这里简化处理，实际应该根据处理结果决定是否提交
        // 如果使用事务，需要更复杂的逻辑
        Ok(())
    }

    fn parse_message(&self, message: &BorrowedMessage<'_>) -> Result<PushDispatchTask> {
        let payload = message.payload().ok_or_else(|| {
            ErrorBuilder::new(ErrorCode::InvalidParameter, "Empty message payload").build_error()
        })?;

        let task: PushDispatchTask = serde_json::from_slice(payload).map_err(|e| {
            ErrorBuilder::new(ErrorCode::InvalidParameter, "Invalid task format")
                .details(e.to_string())
                .build_error()
        })?;

        Ok(task)
    }
}
