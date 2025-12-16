//! UserId 值对象
//!
//! 用户ID的强类型封装

use serde::{Deserialize, Serialize};
use std::fmt;

/// 用户ID值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(String);

impl UserId {
    /// 从字符串创建用户ID（带验证）
    pub fn new(id: String) -> Result<Self, String> {
        if id.is_empty() {
            return Err("UserId cannot be empty".to_string());
        }

        if id.len() > 128 {
            return Err("UserId too long (max 128 characters)".to_string());
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

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<UserId> for String {
    fn from(id: UserId) -> Self {
        id.0
    }
}

impl AsRef<str> for UserId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}
