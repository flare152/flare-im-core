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

/// 判断错误是否可重试
pub trait RetryableError {
    fn is_retryable(&self) -> bool;
}

impl RetryableError for anyhow::Error {
    fn is_retryable(&self) -> bool {
        // 网络错误、超时、临时不可用等可以重试
        let error_str = self.to_string().to_lowercase();
        error_str.contains("timeout")
            || error_str.contains("network")
            || error_str.contains("connection")
            || error_str.contains("temporary")
            || error_str.contains("unavailable")
    }
}

/// 带重试的执行函数
pub async fn execute_with_retry<F, Fut, T>(
    policy: &RetryPolicy,
    mut f: F,
) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, anyhow::Error>>,
{
    let mut last_error = None;

    for attempt in 0..policy.max_attempts {
        match f().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if e.is_retryable() && attempt < policy.max_attempts - 1 {
                    // 可重试的错误，等待后重试
                    let delay = policy.calculate_delay(attempt);
                    tokio::time::sleep(delay).await;
                    last_error = Some(e.to_string());
                    continue;
                } else {
                    // 永久失败或达到最大重试次数
                    return Err(e.to_string());
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "Max retries exceeded".to_string()))
}

