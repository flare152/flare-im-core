//! ACK模块性能基准测试
//! 测试ACK处理的吞吐量和延迟性能

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use flare_im_core::ack::{AckModule, config::AckServiceConfig};
use flare_im_core::ack::redis_manager::{AckStatusInfo, AckStatus, ImportanceLevel};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::runtime::Runtime;
use std::collections::HashMap;

fn bench_ack_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // 初始化数据库连接池
    let db_pool = rt.block_on(async {
        let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/test_flare".to_string());
        PgPool::connect(&db_url).await.unwrap()
    });

    // 初始化ACK服务配置
    let ack_config = AckServiceConfig::default();

    let ack_module: Arc<AckModule> = Arc::new(rt.block_on(async {
        AckModule::new(db_pool, ack_config).await.unwrap()
    }));

    // 创建测试数据
    let test_acks: Vec<AckStatusInfo> = (0..1000)
        .map(|i| AckStatusInfo {
            message_id: format!("msg_{}", i),
            user_id: format!("user_{}", i % 100),
            status: AckStatus::Received,
            timestamp: chrono::Utc::now().timestamp() as u64,
            importance: if i % 10 == 0 {
                ImportanceLevel::High
            } else if i % 3 == 0 {
                ImportanceLevel::Medium
            } else {
                ImportanceLevel::Low
            },
        })
        .collect();

    let mut group = c.benchmark_group("ack_processing");
    group.throughput(Throughput::Elements(1));
    
    group.bench_function("record_and_archive_ack", |b| {
        b.iter(|| {
            let ack_module = Arc::clone(&ack_module);
            let ack_info = test_acks[0].clone();
            rt.block_on(async {
                ack_module.record_and_archive_ack(ack_info, true).await.unwrap();
            });
        })
    });

    group.bench_function("get_ack_status", |b| {
        b.iter(|| {
            let ack_module = Arc::clone(&ack_module);
            rt.block_on(async {
                ack_module.get_ack_status("msg_0", "user_0").await.unwrap();
            });
        })
    });

    group.finish();
}

criterion_group!(benches, bench_ack_processing);
criterion_main!(benches);