//! 推送服务基准测试
//!
//! 测试推送服务的性能指标

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use flare_im_core::metrics::PushServerMetrics;
use std::sync::Arc;
use std::time::Instant;

fn bench_push_server_metrics(c: &mut Criterion) {
    let metrics = Arc::new(PushServerMetrics::new());
    
    let mut group = c.benchmark_group("push_server");
    
    // 测试推送任务处理指标记录
    group.bench_function("record_push_task_processed", |b| {
        b.iter(|| {
            metrics.push_tasks_processed_total
                .with_label_values(&["normal", "tenant-001"])
                .inc();
        })
    });
    
    // 测试在线推送成功指标记录
    group.bench_function("record_online_push_success", |b| {
        b.iter(|| {
            metrics.online_push_success_total
                .with_label_values(&["gateway-001", "tenant-001"])
                .inc();
        })
    });
    
    // 测试不同消息类型的指标记录
    for message_type in &["normal", "notification"] {
        group.bench_with_input(
            BenchmarkId::new("record_push_by_type", message_type),
            message_type,
            |b, &msg_type| {
                b.iter(|| {
                    metrics.push_tasks_processed_total
                        .with_label_values(&[msg_type, "tenant-001"])
                        .inc();
                })
            },
        );
    }
    
    group.finish();
}

fn bench_push_server_duration_tracking(c: &mut Criterion) {
    let metrics = Arc::new(PushServerMetrics::new());
    
    c.bench_function("push_server_duration_tracking", |b| {
        b.iter(|| {
            let start = Instant::now();
            // 模拟推送处理逻辑
            std::hint::black_box(());
            let duration = start.elapsed();
            metrics.push_latency_seconds
                .with_label_values(&["online", "tenant-001"])
                .observe(duration.as_secs_f64());
            black_box(duration)
        })
    });
}

criterion_group!(
    benches,
    bench_push_server_metrics,
    bench_push_server_duration_tracking
);
criterion_main!(benches);

