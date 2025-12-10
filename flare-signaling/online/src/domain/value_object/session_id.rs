//! SessionId 值对象
//! 
//! 会话ID的强类型封装，确保ID格式有效

use serde::{Deserialize, Serialize};
use std::fmt;

/// 会话ID值对象
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// 创建新的会话ID（使用UUID v4）
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// 从字符串创建会话ID（带验证）
    pub fn from_string(id: String) -> Result<Self, String> {
        if id.is_empty() {
            return Err("SessionId cannot be empty".to_string());
        }
        
        // 验证是否为有效的UUID格式
        if uuid::Uuid::parse_str(&id).is_err() {
            return Err(format!("Invalid UUID format: {}", id));
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

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SessionId> for String {
    fn from(id: SessionId) -> Self {
        id.0
    }
}

impl AsRef<str> for SessionId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_id_creation() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_session_id_from_string() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000".to_string();
        let id = SessionId::from_string(uuid_str.clone()).unwrap();
        assert_eq!(id.as_str(), uuid_str);
    }

    #[test]
    fn test_session_id_validation() {
        assert!(SessionId::from_string("".to_string()).is_err());
        assert!(SessionId::from_string("invalid-uuid".to_string()).is_err());
    }
}
