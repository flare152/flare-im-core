//! 值对象（Value Object）
//! 
//! 不可变对象，通过值相等判断，包含验证逻辑

mod session_id;
mod user_id;
mod device_id;
mod device_priority;
mod token_version;
mod connection_quality;

pub use session_id::SessionId;
pub use user_id::UserId;
pub use device_id::DeviceId;
pub use device_priority::DevicePriority;
pub use token_version::TokenVersion;
pub use connection_quality::{ConnectionQuality, QualityLevel, NetworkType};