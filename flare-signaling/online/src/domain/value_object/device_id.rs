//! DeviceId 值对象
//!
//! 设备ID的强类型封装

use serde::{Deserialize, Serialize};
use std::fmt;

/// 设备ID值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(String);

impl DeviceId {
    /// 从字符串创建设备ID（带验证）
    pub fn new(id: String) -> Result<Self, String> {
        if id.is_empty() {
            return Err("DeviceId cannot be empty".to_string());
        }

        if id.len() > 128 {
            return Err("DeviceId too long (max 128 characters)".to_string());
        }

        Ok(Self(id))
    }

    /// 获取内部值的引用
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 消费自身，返回内部值
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<DeviceId> for String {
    fn from(id: DeviceId) -> Self {
        id.0
    }
}

impl AsRef<str> for DeviceId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
