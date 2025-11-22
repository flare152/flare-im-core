//! 命令结构体定义（Command DTO）

use flare_proto::storage::StoreMessageRequest;

/// 处理存储消息命令
#[derive(Debug, Clone)]
pub struct ProcessStoreMessageCommand {
    pub request: StoreMessageRequest,
}
