pub mod device_route_repository;
pub mod entities;
pub mod model;
pub mod repository;
pub mod service;
pub mod value_objects;

pub use device_route_repository::DeviceRouteRepository;
pub use entities::device_route::DeviceRoute;
pub use model::*;
pub use repository::*;
pub use service::*;
pub use value_objects::*;

// 重新导出旧的 model（用于向后兼容）
#[deprecated(note = "Use domain::model::route::Route instead")]
pub use model::RouteLegacy as Route;
