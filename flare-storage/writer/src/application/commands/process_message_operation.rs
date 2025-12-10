//! 处理消息操作命令

use flare_proto::common::{Message, MessageOperation};

/// 处理消息操作命令
#[derive(Debug, Clone)]
pub struct ProcessMessageOperationCommand {
    pub operation: MessageOperation,
    pub message: Message,
}

