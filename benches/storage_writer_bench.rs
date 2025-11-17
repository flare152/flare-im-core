//! 存储写入服务基准测试
//!
//! 测试存储写入服务的性能指标

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flare_im_core::metrics::StorageWriterMetrics;
use std::sync::Arc;
use std::time::Instant;

fn bench_storage_writer_metrics(c: &mut Criterion) {
    let metrics = Arc::new(StorageWriterMetrics::new());
    
    c.bench_function("storage_writer_record_persist", |b| {
        b.iter(|| {
            let start = Instant::now();
            metrics.messages_persisted_total
                .with_label_values(&["tenant-001"])
                .inc();
            let duration = start.elapsed();
            metrics.messages_persisted_duration_seconds.observe(duration.as_secs_f64());
            black_box(duration)
        })
    });
}

fn bench_storage_writer_duration_tracking(c: &mut Criterion) {
    let metrics = Arc::new(StorageWriterMetrics::new());
    
    c.bench_function("storage_writer_duration_tracking", |b| {
        b.iter(|| {
            let start = Instant::now();
            // 模拟消息持久化逻辑
            std::hint::black_box(());
            let duration = start.elapsed();
            metrics.messages_persisted_duration_seconds.observe(duration.as_secs_f64());
            black_box(duration)
        })
    });
}

criterion_group!(
    benches,
    bench_storage_writer_metrics,
    bench_storage_writer_duration_tracking
);
criterion_main!(benches);

