pub mod database;
// Gateway Router 已移至 flare-im-core::gateway
// pub mod gateway_router;
pub mod hook_engine;
pub mod messaging;
pub mod push;
pub mod route;
pub mod signaling;
pub mod storage;

// 新增的轻量级网关基础设施组件
pub mod media;
pub mod hook;
pub mod message;
pub mod online;
pub mod session;

pub use database::{create_db_pool, create_db_pool_from_env};
// Gateway Router 已移至 flare-im-core::gateway
// pub use gateway_router::{DeploymentMode, GatewayRouterConfig, GatewayRouterImpl};
pub use push::GrpcPushClient;
pub use route::RouteServiceClient;
pub use signaling::GrpcSignalingClient;
pub use storage::GrpcStorageClient;

// 新增的轻量级网关基础设施组件导出
pub use media::GrpcMediaClient;
pub use hook::GrpcHookClient;
pub use message::GrpcMessageClient;
pub use online::GrpcOnlineClient;
pub use session::GrpcSessionClient;