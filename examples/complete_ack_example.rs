//! 完整的ACK处理示例
//! 展示ACK模块的所有功能集成使用

use flare_im_core::ack::mod.rs::{AckModule, AckModuleStats};
use flare_im_core::ack::service::AckServiceConfig;
use flare_im_core::ack::redis_manager::{AckStatusInfo, AckStatus, ImportanceLevel};
use flare_im_core::ack::metrics::{AckMetrics, PerformanceConfig};
use prometheus::Registry;
use sqlx::PgPoolOptions;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Flare IM ACK处理完整示例 ===\n");

    // 初始化数据库连接池
    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect("postgresql://localhost/flare_im")
        .await?;

    // 创建Prometheus注册表
    let registry = Registry::new();

    // 初始化监控指标
    let metrics = Arc::new(AckMetrics::new(&registry)?);

    // 配置ACK服务
    let ack_config = AckServiceConfig {
        redis_url: "redis://127.0.0.1:6379/".to_string(),
        redis_ttl: 3600,
        cache_capacity: 10000,
        batch_interval_ms: 100,
        batch_size: 100,
    };

    // 创建ACK模块
    let ack_module = AckModule::new(db_pool, ack_config).await?;
    println!("✓ ACK模块初始化成功");

    // 模拟处理不同重要性的ACK
    let ack_infos = vec![
        // 高重要性ACK - 需要立即持久化
        AckStatusInfo {
            message_id: "high_priority_msg_1".to_string(),
            user_id: "user_1".to_string(),
            status: AckStatus::Received,
            timestamp: get_current_timestamp(),
            importance: ImportanceLevel::High,
        },
        // 中等重要性ACK - 批量处理
        AckStatusInfo {
            message_id: "medium_priority_msg_1".to_string(),
            user_id: "user_2".to_string(),
            status: AckStatus::Processed,
            timestamp: get_current_timestamp(),
            importance: ImportanceLevel::Medium,
        },
        // 低重要性ACK - 仅内存缓存
        AckStatusInfo {
            message_id: "low_priority_msg_1".to_string(),
            user_id: "user_3".to_string(),
            status: AckStatus::Received,
            timestamp: get_current_timestamp(),
            importance: ImportanceLevel::Low,
        },
    ];

    // 处理ACK并根据需要归档
    for (i, ack_info) in ack_infos.iter().enumerate() {
        let should_archive = matches!(ack_info.importance, ImportanceLevel::High);
        ack_module.record_and_archive_ack(ack_info.clone(), should_archive).await?;
        println!("✓ 处理ACK #{}: 消息={}, 用户={}, 重要性={:?}", 
            i + 1, 
            ack_info.message_id, 
            ack_info.user_id, 
            ack_info.importance
        );
    }

    // 等待批处理完成
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // 查询ACK状态
    println!("\n--- 查询ACK状态 ---");
    for ack_info in &ack_infos {
        let status = ack_module.get_ack_status(&ack_info.message_id, &ack_info.user_id).await?;
        if let Some(status) = status {
            println!("消息 '{}' 的ACK状态: {:?}", ack_info.message_id, status.status);
        } else {
            println!("未找到消息 '{}' 的ACK状态", ack_info.message_id);
        }
    }

    // 检查ACK是否存在
    println!("\n--- 检查ACK存在性 ---");
    for ack_info in &ack_infos {
        let exists = ack_module.exists_ack(&ack_info.message_id, &ack_info.user_id).await?;
        println!("消息 '{}' 的ACK是否存在: {}", ack_info.message_id, exists);
    }

    // 获取模块统计信息
    println!("\n--- 模块统计信息 ---");
    let stats = ack_module.get_stats().await?;
    print_stats(&stats);

    // 获取Prometheus指标
    println!("\n--- Prometheus指标 ---");
    let metrics_data = metrics.get_metrics_data(&registry)?;
    println!("{}", metrics_data);

    // 性能优化示例
    println!("\n--- 性能优化示例 ---");
    let performance_config = PerformanceConfig::default();
    println!("性能配置: {:?}", performance_config);

    // 模拟大量ACK处理
    println!("\n--- 批量处理性能测试 ---");
    let start_time = SystemTime::now();
    for i in 0..1000 {
        let ack_info = AckStatusInfo {
            message_id: format!("bulk_msg_{}", i),
            user_id: format!("bulk_user_{}", i % 100),
            status: AckStatus::Received,
            timestamp: get_current_timestamp(),
            importance: if i % 10 == 0 { 
                ImportanceLevel::High 
            } else if i % 3 == 0 { 
                ImportanceLevel::Medium 
            } else { 
                ImportanceLevel::Low 
            },
        };
        
        let should_archive = matches!(ack_info.importance, ImportanceLevel::High);
        ack_module.record_and_archive_ack(ack_info, should_archive).await?;
    }
    
    let duration = SystemTime::now().duration_since(start_time)?.as_millis();
    println!("处理1000个ACK耗时: {} ms", duration);

    // 再次获取统计信息
    println!("\n--- 更新后的统计信息 ---");
    let stats = ack_module.get_stats().await?;
    print_stats(&stats);

    println!("\n=== 示例完成 ===");
    Ok(())
}

/// 获取当前时间戳
fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// 打印统计信息
fn print_stats(stats: &AckModuleStats) {
    println!("服务统计:");
    println!("  - 缓存大小: {}", stats.service_stats.cache_size);
    println!("  - 批处理队列大小: {}", stats.service_stats.batch_queue_size);
    println!("  - Redis已使用内存: {} bytes", stats.service_stats.redis_stats.used_memory);
    
    println!("归档统计:");
    println!("  - 总记录数: {}", stats.archive_stats.total_records);
    println!("  - 高重要性记录: {}", stats.archive_stats.high_importance);
    println!("  - 中等重要性记录: {}", stats.archive_stats.medium_importance);
    println!("  - 低重要性记录: {}", stats.archive_stats.low_importance);
}