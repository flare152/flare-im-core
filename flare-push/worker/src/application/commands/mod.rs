//! 命令结构体定义（Command DTO）

use crate::domain::model::PushDispatchTask;

/// 执行推送任务命令
#[derive(Debug, Clone)]
pub struct ExecutePushTaskCommand {
    /// 推送任务
    pub task: PushDispatchTask,
}

/// 批量执行推送任务命令
#[derive(Debug, Clone)]
pub struct BatchExecutePushTasksCommand {
    /// 批量任务
    pub tasks: Vec<PushDispatchTask>,
}

