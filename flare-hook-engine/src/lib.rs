//! # Flare Hook Engine
//!
//! Hook引擎是 Flare IM 基座的核心基础设施组件，提供统一的Hook配置管理、执行调度和监控统计能力。
//!
//! ## 核心职责
//!
//! - **配置管理**：支持配置文件、动态API、配置中心三种配置方式
//! - **执行调度**：按优先级执行Hook，支持并发执行和超时控制
//! - **监控统计**：收集Hook执行指标，提供监控和告警能力
//! - **扩展机制**：支持gRPC、WebHook、Local Plugin三种扩展方式
//!
//! ## 架构设计
//!
//! Hook引擎采用DDD + CQRS架构：
//! - **domain层**：领域模型、仓储接口、领域服务
//! - **application层**：用例编排与服务门面
//! - **infrastructure层**：配置加载、适配器、持久化
//! - **interface层**：gRPC服务接口
//! - **service层**：应用启动和依赖注入

pub mod application;
pub mod domain;
pub mod infrastructure;
pub mod interface;
pub mod service;

// Re-export commonly used types
pub use domain::model::{
    ExecutionMode, HookConfig, HookExecutionPlan, HookExecutionResult, HookStatistics,
};
pub use infrastructure::config::{ConfigLoader, ConfigWatcher};
pub use service::ApplicationBootstrap;
