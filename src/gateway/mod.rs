//! Gateway Router 模块
//!
//! 跨地区网关路由组件，根据 gateway_id 路由到对应的 Access Gateway。
//! 支持单地区/多地区自适应部署。

pub mod router;

pub use router::{GatewayRouter, GatewayRouterConfig, GatewayRouterError, GatewayRouterTrait};
