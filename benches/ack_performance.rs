//! ACK模块性能基准测试
//! 测试ACK处理的性能表现

use criterion::{black_box, criterion_group, criterion_main, Bencher, Criterion};
use flare_im_core::ack::redis_manager::{RedisAckManager, AckStatusInfo, AckStatus, ImportanceLevel};
use flare_im_core::ack::service::{AckService, AckServiceConfig};
use std::sync::Arc;
use tokio::runtime::Runtime;

fn bench_redis_ack_storage(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // 注意：这需要一个运行中的Redis实例
    let redis_manager = rt.block_on(async {
        RedisAckManager::new("redis://127.0.0.1/", 3600).unwrap()
    });
    
    let redis_manager = Arc::new(redis_manager);
    
    c.bench_function("redis_ack_storage", |b: &mut Bencher| {
        b.iter(|| {
            let ack_info = AckStatusInfo {
                message_id: "test_msg".to_string(),
                user_id: "test_user".to_string(),
                status: AckStatus::Received,
                timestamp: 1234567890,
                importance: ImportanceLevel::High,
            };
            
            rt.block_on(async {
                redis_manager.store_ack_status(black_box(&ack_info)).await.unwrap();
            });
        })
    });
}

fn bench_redis_ack_retrieval(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    // 注意：这需要一个运行中的Redis实例
    let redis_manager = rt.block_on(async {
        RedisAckManager::new("redis://127.0.0.1/", 3600).unwrap()
    });
    
    let redis_manager = Arc::new(redis_manager);
    
    // 预先存储一些数据
    rt.block_on(async {
        let ack_info = AckStatusInfo {
            message_id: "test_msg_retrieve".to_string(),
            user_id: "test_user_retrieve".to_string(),
            status: AckStatus::Received,
            timestamp: 1234567890,
            importance: ImportanceLevel::High,
        };
        redis_manager.store_ack_status(&ack_info).await.unwrap();
    });
    
    c.bench_function("redis_ack_retrieval", |b: &mut Bencher| {
        b.iter(|| {
            rt.block_on(async {
                let result = redis_manager.get_ack_status(
                    black_box("test_msg_retrieve"), 
                    black_box("test_user_retrieve")
                ).await.unwrap();
                black_box(result);
            });
        })
    });
}

fn bench_ack_service_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let config = AckServiceConfig::default();
    
    let ack_service = rt.block_on(async {
        AckService::new(config).await.unwrap()
    });
    
    let ack_service = Arc::new(ack_service);
    
    c.bench_function("ack_service_processing", |b: &mut Bencher| {
        b.iter(|| {
            let ack_info = AckStatusInfo {
                message_id: "test_msg_service".to_string(),
                user_id: "test_user_service".to_string(),
                status: AckStatus::Received,
                timestamp: 1234567890,
                importance: ImportanceLevel::High,
            };
            
            rt.block_on(async {
                ack_service.record_ack(black_box(ack_info)).await.unwrap();
            });
        })
    });
}

fn bench_ack_service_batch_processing(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    
    let config = AckServiceConfig {
        batch_size: 10,
        ..Default::default()
    };
    
    let ack_service = rt.block_on(async {
        AckService::new(config).await.unwrap()
    });
    
    let ack_service = Arc::new(ack_service);
    
    c.bench_function("ack_service_batch_processing", |b: &mut Bencher| {
        b.iter(|| {
            for i in 0..10 {
                let ack_info = AckStatusInfo {
                    message_id: format!("test_msg_batch_{}", i),
                    user_id: format!("test_user_batch_{}", i),
                    status: AckStatus::Received,
                    timestamp: 1234567890 + i,
                    importance: ImportanceLevel::Medium,
                };
                
                rt.block_on(async {
                    ack_service.record_ack(black_box(ack_info)).await.unwrap();
                });
            }
        })
    });
}

criterion_group!(
    benches,
    bench_redis_ack_storage,
    bench_redis_ack_retrieval,
    bench_ack_service_processing,
    bench_ack_service_batch_processing
);

criterion_main!(benches);