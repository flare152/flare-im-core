//! 推送重试机制（指数退避策略）

use std::time::Duration;

/// 重试策略配置
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// 最大重试次数
    pub max_attempts: u32,
    /// 初始延迟（毫秒）
    pub initial_delay_ms: u64,
    /// 最大延迟（毫秒）
    pub max_delay_ms: u64,
    /// 退避倍数
    pub backoff_multiplier: f64,
}

impl RetryPolicy {
    /// 创建默认重试策略
    pub fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay_ms: 100,
            max_delay_ms: 5000,
            backoff_multiplier: 2.0,
        }
    }

    /// 从配置创建重试策略
    pub fn from_config(
        max_attempts: u32,
        initial_delay_ms: u64,
        max_delay_ms: u64,
        backoff_multiplier: f64,
    ) -> Self {
        Self {
            max_attempts,
            initial_delay_ms,
            max_delay_ms,
            backoff_multiplier,
        }
    }

    /// 计算重试延迟（指数退避）
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        let delay_ms = (self.initial_delay_ms as f64 * self.backoff_multiplier.powi(attempt as i32))
            .min(self.max_delay_ms as f64) as u64;
        Duration::from_millis(delay_ms)
    }
}

/// 重试结果
#[derive(Debug, Clone)]
pub enum RetryResult<T> {
    /// 成功
    Success(T),
    /// 临时失败，可以重试
    RetryableError(String),
    /// 永久失败，不应重试
    PermanentError(String),
}

/// 错误类型（用于智能重试判断）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorType {
    /// 网络错误（可重试）
    Network,
    /// 超时错误（可重试）
    Timeout,
    /// 临时不可用（可重试）
    TemporaryUnavailable,
    /// 用户离线（不可重试，需要重新查询在线状态）
    UserOffline,
    /// 认证失败（不可重试）
    AuthenticationFailed,
    /// 参数错误（不可重试）
    InvalidParameter,
    /// 其他错误（根据错误信息判断）
    Other,
}

/// 判断错误是否可重试
pub trait RetryableError {
    fn is_retryable(&self) -> bool;
    fn error_type(&self) -> ErrorType;
}

impl RetryableError for anyhow::Error {
    fn is_retryable(&self) -> bool {
        matches!(
            self.error_type(),
            ErrorType::Network | ErrorType::Timeout | ErrorType::TemporaryUnavailable
        )
    }

    fn error_type(&self) -> ErrorType {
        let error_str = self.to_string().to_lowercase();

        if error_str.contains("user offline") || error_str.contains("users offline") {
            ErrorType::UserOffline
        } else if error_str.contains("timeout") {
            ErrorType::Timeout
        } else if error_str.contains("network") || error_str.contains("connection") {
            ErrorType::Network
        } else if error_str.contains("temporary") || error_str.contains("unavailable") {
            ErrorType::TemporaryUnavailable
        } else if error_str.contains("auth") || error_str.contains("unauthorized") {
            ErrorType::AuthenticationFailed
        } else if error_str.contains("invalid") || error_str.contains("bad request") {
            ErrorType::InvalidParameter
        } else {
            ErrorType::Other
        }
    }
}

/// 带智能重试的执行函数
///
/// 智能重试策略：
/// - 网络错误、超时、临时不可用：指数退避重试
/// - 用户离线：立即返回，不重试（需要重新查询在线状态）
/// - 认证失败、参数错误：立即返回，不重试
pub async fn execute_with_retry<F, Fut, T>(policy: &RetryPolicy, mut f: F) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
{
    let mut last_error = None;

    for attempt in 0..policy.max_attempts {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                let error_type = e.error_type();

                // 根据错误类型决定是否重试
                match error_type {
                    ErrorType::UserOffline
                    | ErrorType::AuthenticationFailed
                    | ErrorType::InvalidParameter => {
                        // 永久失败，不重试
                        return Err(e.to_string());
                    }
                    ErrorType::Network | ErrorType::Timeout | ErrorType::TemporaryUnavailable => {
                        // 可重试的错误
                        if attempt < policy.max_attempts - 1 {
                            let delay = policy.calculate_delay(attempt);
                            tracing::debug!(
                                attempt = attempt + 1,
                                max_attempts = policy.max_attempts,
                                delay_ms = delay.as_millis(),
                                error_type = ?error_type,
                                "Retrying after error"
                            );
                            tokio::time::sleep(delay).await;
                            last_error = Some(e.to_string());
                            continue;
                        } else {
                            // 达到最大重试次数
                            return Err(format!("Max retries exceeded: {}", e));
                        }
                    }
                    ErrorType::Other => {
                        // 其他错误，根据 is_retryable 判断
                        if e.is_retryable() && attempt < policy.max_attempts - 1 {
                            let delay = policy.calculate_delay(attempt);
                            tracing::debug!(
                                attempt = attempt + 1,
                                max_attempts = policy.max_attempts,
                                delay_ms = delay.as_millis(),
                                "Retrying after retryable error"
                            );
                            tokio::time::sleep(delay).await;
                            last_error = Some(e.to_string());
                            continue;
                        } else {
                            return Err(e.to_string());
                        }
                    }
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "Max retries exceeded".to_string()))
}
