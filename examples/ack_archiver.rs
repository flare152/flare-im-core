//! ACK异步归档机制示例
//! 展示如何使用ACK归档器进行审计和分析

use flare_im_core::ack::archiver::{AckArchiver, AckArchiveRecord, init_ack_archive_table};
use sqlx::PgPoolOptions;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化数据库连接池
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgresql://localhost/flare_im")
        .await?;

    // 初始化ACK归档表
    init_ack_archive_table(&db_pool).await?;

    // 创建ACK归档器
    let archiver = AckArchiver::new(db_pool, 1000);

    // 模拟一些ACK记录
    let ack_records = vec![
        AckArchiveRecord {
            message_id: "msg_001".to_string(),
            user_id: "user_001".to_string(),
            ack_type: "client".to_string(),
            ack_status: "received".to_string(),
            timestamp: get_current_timestamp(),
            importance_level: 3, // 高重要性
            metadata: Some(serde_json::json!({
                "device_id": "device_001",
                "platform": "iOS",
                "app_version": "1.0.0"
            })),
            archived_at: get_current_timestamp(),
        },
        AckArchiveRecord {
            message_id: "msg_002".to_string(),
            user_id: "user_002".to_string(),
            ack_type: "push".to_string(),
            ack_status: "delivered".to_string(),
            timestamp: get_current_timestamp() - 1000, // 1秒前
            importance_level: 2, // 中等重要性
            metadata: Some(serde_json::json!({
                "push_provider": "APNs",
                "retry_count": 0
            })),
            archived_at: get_current_timestamp(),
        },
        AckArchiveRecord {
            message_id: "msg_003".to_string(),
            user_id: "user_003".to_string(),
            ack_type: "storage".to_string(),
            ack_status: "stored".to_string(),
            timestamp: get_current_timestamp() - 2000, // 2秒前
            importance_level: 1, // 低重要性
            metadata: None,
            archived_at: get_current_timestamp(),
        },
    ];

    // 归档ACK记录
    for record in ack_records {
        archiver.archive_ack(record).await?;
        println!("Archived ACK record for message: {}", record.message_id);
    }

    // 等待一段时间让归档任务完成
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // 查询归档的ACK记录
    let archived_records = archiver.query_archived_acks(
        None,           // 不限制消息ID
        None,           // 不限制用户ID
        None,           // 不限制开始时间
        None,           // 不限制结束时间
        Some(10),       // 限制返回10条记录
    ).await?;

    println!("\n=== Archived ACK Records ===");
    for record in archived_records {
        println!(
            "Message: {}, User: {}, Type: {}, Status: {}, Time: {}, Importance: {}",
            record.message_id,
            record.user_id,
            record.ack_type,
            record.ack_status,
            record.timestamp,
            record.importance_level
        );
        
        if let Some(metadata) = record.metadata {
            println!("  Metadata: {}", serde_json::to_string_pretty(&metadata)?);
        }
    }

    // 获取归档统计信息
    let stats = archiver.get_archive_stats().await?;
    println!("\n=== Archive Statistics ===");
    println!("Total records: {}", stats.total_records);
    println!("High importance: {}", stats.high_importance);
    println!("Medium importance: {}", stats.medium_importance);
    println!("Low importance: {}", stats.low_importance);
    println!("Latest timestamp: {}", stats.latest_timestamp);

    Ok(())
}

/// 获取当前时间戳
fn get_current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}