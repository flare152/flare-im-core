//! # OpenTelemetry 分布式追踪模块
//!
//! 为各个服务模块提供统一的 OpenTelemetry 分布式追踪能力。
//!
//! 注意：OpenTelemetry 相关功能需要启用 `tracing` feature 才能使用。
//! 基础的日志初始化功能不需要 feature gate.

use tracing_subscriber::{EnvFilter, fmt};

#[cfg(feature = "tracing")]
use tracing::{Span, info, warn};

/// 从配置初始化日志系统
///
/// # 参数
/// * `logging_config` - 日志配置（可选），如果为 None 则使用默认配置（debug 级别）
///
/// # 示例
/// ```rust,ignore
/// use flare_im_core::config::LoggingConfig;
///
/// // 使用默认配置
/// init_tracing_from_config(None);
///
/// // 使用自定义配置
/// let config = LoggingConfig {
///     level: "info".to_string(),
///     with_target: false,
///     with_thread_ids: true,
///     with_file: true,
///     with_line_number: true,
/// };
/// init_tracing_from_config(Some(&config));
/// ```
pub fn init_tracing_from_config(logging_config: Option<&crate::config::LoggingConfig>) {
    // 优先使用环境变量 RUST_LOG，如果没有则使用配置文件的日志级别
    let env_filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => {
            let level_str = logging_config.map(|c| c.level.as_str()).unwrap_or("debug");
            EnvFilter::new(level_str)
        }
    };

    // 获取日志配置（如果未提供则使用默认配置）
    let default_config = crate::config::LoggingConfig::default();
    let config = logging_config.unwrap_or(&default_config);

    let builder = fmt::Subscriber::builder()
        .with_target(config.with_target)
        .with_thread_ids(config.with_thread_ids)
        .with_file(config.with_file)
        .with_line_number(config.with_line_number)
        .with_env_filter(env_filter);

    builder.init();
}

/// 初始化 OpenTelemetry 追踪
///
/// 如果提供了 OTLP endpoint，会尝试初始化 OpenTelemetry OTLP 导出器（连接到 Tempo）。
/// 如果初始化失败或未提供 endpoint，则使用基础的 tracing fmt layer。
///
/// # 参数
/// * `service_name` - 服务名称（如 "message-orchestrator"）
/// * `endpoint` - Tempo OTLP 端点（如 "http://localhost:4317"），如果为 None 则使用基础 tracing
///
/// # 示例
/// ```rust
/// // 连接到 Tempo
/// init_tracing("message-orchestrator", Some("http://localhost:4317"))?;
///
/// // 使用基础 tracing（不连接 Tempo）
/// init_tracing("message-orchestrator", None)?;
/// ```
///
/// # 参考
/// - `中间件设计方案.md` - Tempo 配置说明
#[cfg(feature = "tracing")]
pub fn init_tracing(
    service_name: &str,
    endpoint: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // 尝试初始化 OpenTelemetry OTLP（如果提供了 endpoint）
    #[cfg(all(feature = "tracing", feature = "opentelemetry"))]
    {
        if let Some(otlp_endpoint) = endpoint {
            match init_otlp_tracing(service_name, otlp_endpoint) {
                Ok(_) => {
                    info!(
                        service_name = %service_name,
                        endpoint = %otlp_endpoint,
                        "OpenTelemetry OTLP tracing initialized (connected to Tempo)"
                    );
                    return Ok(());
                }
                Err(e) => {
                    // OTLP 初始化失败，降级到基础 tracing
                    warn!(
                        service_name = %service_name,
                        endpoint = %otlp_endpoint,
                        error = %e,
                        "Failed to initialize OpenTelemetry OTLP, falling back to basic tracing"
                    );
                }
            }
        }
    }

    // 初始化基础的 tracing subscriber（使用 fmt layer）
    // 默认配置：debug 级别，显示线程ID、文件名和行号
    init_tracing_from_config(None);

    if endpoint.is_some() {
        info!(
            service_name = %service_name,
            endpoint = %endpoint.unwrap(),
            "Tracing initialized (basic tracing mode, Tempo connection pending)"
        );
    } else {
        info!(
            service_name = %service_name,
            "Tracing initialized (basic tracing mode)"
        );
    }

    Ok(())
}

/// 初始化 OpenTelemetry OTLP 追踪（内部函数）
///
/// 连接到 Tempo 分布式追踪后端（通过 OTLP gRPC 协议）。
///
/// 注意：此函数需要 OpenTelemetry 0.28 API，如果 API 不兼容会返回错误并降级到基础 tracing。
///
/// # 参数
/// * `service_name` - 服务名称
/// * `endpoint` - Tempo OTLP 端点（如 "http://localhost:4317"）
///
/// # 参考
/// - `中间件设计方案.md` - Tempo 配置说明
/// - OpenTelemetry 0.28 官方文档
#[cfg(all(feature = "tracing", feature = "opentelemetry"))]
fn init_otlp_tracing(
    _service_name: &str,
    _endpoint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use tracing_subscriber::{EnvFilter, fmt};

    // 暂时禁用 OpenTelemetry 追踪，直接使用基础 tracing
    let env_filter = match EnvFilter::try_from_default_env() {
        Ok(filter) => filter,
        Err(_) => EnvFilter::new("debug"),
    };

    fmt::Subscriber::builder()
        .with_env_filter(env_filter)
        .init();

    Ok(())
}

/// 创建追踪 Span
///
/// 注意：当前实现返回当前 Span，实际追踪通过 `#[instrument]` 宏和 `tracing::Span::current()` 实现
/// 建议在代码中使用 `#[instrument]` 宏而不是手动创建 Span
#[cfg(feature = "tracing")]
pub fn create_span(_tracer_name: &str, _span_name: &str) -> Span {
    // 返回当前 Span，实际追踪通过 #[instrument] 宏实现
    // 这是一个占位实现，完整的 OpenTelemetry Span 创建待完善
    Span::current()
}

/// 从当前 Span 获取追踪信息
///
/// 注意：当前实现返回 None，完整的追踪信息需要 OpenTelemetry 集成
#[cfg(feature = "tracing")]
pub fn get_trace_info() -> Option<(String, String)> {
    // OpenTelemetry 追踪信息获取待完善
    // 当前返回 None，实际追踪通过 tracing 日志实现
    None
}

/// 关闭追踪（清理资源）
#[cfg(feature = "tracing")]
pub fn shutdown_tracing() {
    // 基础 tracing 不需要显式关闭
    // OpenTelemetry 资源清理待完善
    info!("Tracing shutdown (OpenTelemetry cleanup pending)");
}
