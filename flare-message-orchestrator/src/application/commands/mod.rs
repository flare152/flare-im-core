//! 命令结构体定义（Command DTO）

use flare_proto::storage::StoreMessageRequest;

/// 存储消息命令
#[derive(Debug, Clone)]
pub struct StoreMessageCommand {
    /// 原始请求
    pub request: StoreMessageRequest,
}

/// 批量存储消息命令
#[derive(Debug, Clone)]
pub struct BatchStoreMessageCommand {
    /// 批量请求
    pub requests: Vec<StoreMessageRequest>,
}
