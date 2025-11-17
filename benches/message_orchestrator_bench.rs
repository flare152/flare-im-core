//! 消息编排服务基准测试
//!
//! 测试消息编排服务的性能指标

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use flare_im_core::metrics::MessageOrchestratorMetrics;
use std::sync::Arc;
use std::time::Instant;

fn bench_message_orchestrator_metrics(c: &mut Criterion) {
    let metrics = Arc::new(MessageOrchestratorMetrics::new());
    
    let mut group = c.benchmark_group("message_orchestrator");
    
    // 测试指标记录性能
    group.bench_function("record_message_sent", |b| {
        b.iter(|| {
            let start = Instant::now();
            metrics.messages_sent_total
                .with_label_values(&["normal", "tenant-001"])
                .inc();
            let duration = start.elapsed();
            metrics.messages_sent_duration_seconds.observe(duration.as_secs_f64());
            black_box(duration)
        })
    });
    
    // 测试不同消息类型的指标记录
    for message_type in &["normal", "notification"] {
        group.bench_with_input(
            BenchmarkId::new("record_message_by_type", message_type),
            message_type,
            |b, &msg_type| {
                b.iter(|| {
                    metrics.messages_sent_total
                        .with_label_values(&[msg_type, "tenant-001"])
                        .inc();
                })
            },
        );
    }
    
    group.finish();
}

fn bench_message_orchestrator_duration_tracking(c: &mut Criterion) {
    let metrics = Arc::new(MessageOrchestratorMetrics::new());
    
    c.bench_function("message_orchestrator_duration_tracking", |b| {
        b.iter(|| {
            let start = Instant::now();
            // 模拟消息处理逻辑
            std::hint::black_box(());
            let duration = start.elapsed();
            metrics.messages_sent_duration_seconds.observe(duration.as_secs_f64());
            black_box(duration)
        })
    });
}

criterion_group!(
    benches,
    bench_message_orchestrator_metrics,
    bench_message_orchestrator_duration_tracking
);
criterion_main!(benches);

