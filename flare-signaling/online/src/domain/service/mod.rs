//! 领域服务（Domain Service）

pub mod device_manager_service;
pub mod online_status_service;
pub mod subscription_service;
pub mod user_service;

pub use device_manager_service::DeviceManagerService;
pub use online_status_service::OnlineStatusService as OnlineStatusDomainService;
pub use subscription_service::SubscriptionService as SubscriptionDomainService;
pub use user_service::UserService as UserDomainService;
