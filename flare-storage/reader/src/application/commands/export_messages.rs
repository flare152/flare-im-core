//! 导出消息命令服务

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use flare_proto::common::TimeRange;
use flare_proto::storage::{ExportMessagesRequest, ExportMessagesResponse};
use std::sync::Arc;
use uuid::Uuid;

use crate::config::StorageReaderConfig;
use crate::domain::MessageStorage;
use crate::infrastructure::persistence::mongo::MongoMessageStorage;

pub struct ExportMessagesService {
    config: Arc<StorageReaderConfig>,
    storage: Arc<MongoMessageStorage>,
}

impl ExportMessagesService {
    pub async fn new(config: Arc<StorageReaderConfig>) -> Result<Self> {
        let storage = if let Some(url) = &config.mongo_url {
            MongoMessageStorage::new(url, &config.mongo_database)
                .await
                .unwrap_or_default()
        } else {
            MongoMessageStorage::default()
        };

        Ok(Self {
            config,
            storage: Arc::new(storage),
        })
    }

    pub async fn execute(&self, req: ExportMessagesRequest) -> Result<ExportMessagesResponse> {
        // 生成导出任务ID
        let export_task_id = format!("export-{}", Uuid::new_v4());

        // 解析时间范围
        let (start_time, end_time) = if let Some(time_range) = &req.time_range {
            let start = time_range
                .start_time
                .as_ref()
                .and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts.seconds, ts.nanos as u32).single());
            let end = time_range
                .end_time
                .as_ref()
                .and_then(|ts| chrono::TimeZone::timestamp_opt(&Utc, ts.seconds, ts.nanos as u32).single());
            (start, end)
        } else {
            // 如果没有指定时间范围，使用默认范围
            let end = Some(Utc::now());
            let start = Some(Utc::now() - chrono::Duration::seconds(self.config.default_range_seconds));
            (start, end)
        };

        // 启动后台导出任务（简化实现：同步导出，实际应该异步执行）
        let export_task_id_clone = export_task_id.clone();
        let session_id = req.session_id.clone();
        let filters = req.filters.clone();
        let storage = Arc::clone(&self.storage);
        
        tokio::spawn(async move {
            tracing::info!(export_task_id = %export_task_id_clone, "Starting message export task");
            
            // 执行搜索以获取消息
            let messages = storage
                .search_messages(&filters, start_time, end_time, 10000)
                .await;

            match messages {
                Ok(msgs) => {
                    tracing::info!(
                        export_task_id = %export_task_id_clone,
                        message_count = msgs.len(),
                        "Export task completed"
                    );
                    // TODO: 实际实现应该将消息导出到文件或对象存储
                    // 这里只是记录日志，实际生产环境应该：
                    // 1. 将消息序列化为 CSV/JSON 格式
                    // 2. 上传到对象存储（MinIO/S3）
                    // 3. 返回下载链接
                }
                Err(err) => {
                    tracing::error!(
                        export_task_id = %export_task_id_clone,
                        error = ?err,
                        "Export task failed"
                    );
                }
            }
        });

        Ok(ExportMessagesResponse {
            export_task_id,
            status: Some(flare_server_core::error::ok_status()),
        })
    }
}

