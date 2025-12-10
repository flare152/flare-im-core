//! TokenVersion 值对象
//! 
//! Token版本号，用于强制下线旧客户端
//! 
//! 设计参考：微信、钉钉的"顶号"机制
//! - 同一用户在新设备登录时，可指定更高的 token_version
//! - 系统自动踢出 token_version 更低的设备
//! - 适用场景：账号被盗、强制升级客户端

use serde::{Deserialize, Serialize};
use std::fmt;

/// Token版本值对象
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TokenVersion(i64);

impl TokenVersion {
    /// 创建Token版本
    pub fn new(version: i64) -> Result<Self, String> {
        if version < 0 {
            return Err("TokenVersion cannot be negative".to_string());
        }
        Ok(Self(version))
    }

    /// 获取版本号
    pub fn value(&self) -> i64 {
        self.0
    }

    /// 判断是否比另一个版本更新
    pub fn is_newer_than(&self, other: &TokenVersion) -> bool {
        self.0 > other.0
    }

    /// 判断是否应该踢出旧版本
    /// 
    /// 规则：
    /// - 如果当前版本为0，不踢出（向后兼容）
    /// - 如果旧版本为0，不踢出（向后兼容）
    /// - 如果当前版本 > 旧版本，踢出
    pub fn should_kick(&self, old_version: &TokenVersion) -> bool {
        if self.0 == 0 || old_version.0 == 0 {
            return false; // 版本0表示未启用版本控制
        }
        self.0 > old_version.0
    }

    /// 零版本（表示未启用版本控制）
    pub fn zero() -> Self {
        Self(0)
    }
}

impl Default for TokenVersion {
    fn default() -> Self {
        Self::zero()
    }
}

impl fmt::Display for TokenVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.0)
    }
}

impl From<i64> for TokenVersion {
    fn from(version: i64) -> Self {
        Self::new(version).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_version_creation() {
        assert!(TokenVersion::new(1).is_ok());
        assert!(TokenVersion::new(-1).is_err());
    }

    #[test]
    fn test_token_version_comparison() {
        let v1 = TokenVersion::new(1).unwrap();
        let v2 = TokenVersion::new(2).unwrap();
        
        assert!(v2.is_newer_than(&v1));
        assert!(!v1.is_newer_than(&v2));
    }

    #[test]
    fn test_token_version_kick_logic() {
        let v0 = TokenVersion::zero();
        let v1 = TokenVersion::new(1).unwrap();
        let v2 = TokenVersion::new(2).unwrap();
        
        // 版本0不踢出
        assert!(!v1.should_kick(&v0));
        assert!(!v0.should_kick(&v1));
        
        // 高版本踢出低版本
        assert!(v2.should_kick(&v1));
        assert!(!v1.should_kick(&v2));
    }
}
