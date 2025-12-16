//! 操作分类器
//!
//! 根据架构优化方案，将消息操作分为：
//! - 简单操作：直接委托给 Storage Reader/Writer
//! - 复杂操作：需要 Orchestrator 编排处理
//!
//! Kafka 策略：
//! - 需要 Kafka：需要广播、需要全端同步、改变全局状态、创建新消息
//! - 不需要 Kafka：仅用户维度、仅本地状态、不需要审计

use flare_proto::common::{DeleteType, OperationType, message_operation::OperationData};

/// 操作分类器
pub struct OperationClassifier;

impl OperationClassifier {
    /// 判断操作是否需要编排
    ///
    /// 复杂操作需要 Orchestrator 进行编排，包括：
    /// - 权限验证
    /// - 状态更新
    /// - 事件发布
    /// - Hook 执行
    pub fn requires_orchestration(operation_type: i32) -> bool {
        matches!(
            OperationType::try_from(operation_type).ok(),
            Some(OperationType::Recall)
                | Some(OperationType::Edit)
                | Some(OperationType::Forward)
                | Some(OperationType::Reply)
                | Some(OperationType::Quote)
                | Some(OperationType::ThreadReply)
        )
    }

    /// 判断操作是否可以简单委托
    ///
    /// 简单操作可以直接委托给 Storage Reader/Writer，包括：
    /// - 标记已读
    /// - 添加/移除反应
    /// - 置顶/取消置顶
    /// - 收藏/取消收藏
    /// - 标记消息
    /// - 删除消息（软删除）
    pub fn can_delegate_directly(operation_type: i32) -> bool {
        matches!(
            OperationType::try_from(operation_type).ok(),
            Some(OperationType::Read)
                | Some(OperationType::ReactionAdd)
                | Some(OperationType::ReactionRemove)
                | Some(OperationType::Pin)
                | Some(OperationType::Unpin)
                | Some(OperationType::Favorite)
                | Some(OperationType::Unfavorite)
                | Some(OperationType::Mark)
                | Some(OperationType::Delete) // 软删除可以委托
        )
    }

    /// 判断操作是否需要创建新消息
    ///
    /// 这些操作会创建新的消息，需要完整的存储编排流程
    pub fn requires_message_creation(operation_type: i32) -> bool {
        matches!(
            OperationType::try_from(operation_type).ok(),
            Some(OperationType::Reply)
                | Some(OperationType::Forward)
                | Some(OperationType::Quote)
                | Some(OperationType::ThreadReply)
        )
    }

    /// 获取操作类别
    pub fn get_operation_category(operation_type: i32) -> OperationCategory {
        if Self::requires_orchestration(operation_type) {
            OperationCategory::Complex
        } else if Self::can_delegate_directly(operation_type) {
            OperationCategory::Simple
        } else {
            OperationCategory::Unknown
        }
    }

    /// 判断操作是否需要 Kafka 发布
    ///
    /// 需要 Kafka 的情况：
    /// 1. 需要广播给其他用户
    /// 2. 需要全端同步
    /// 3. 改变全局状态
    /// 4. 创建新消息
    pub fn requires_kafka(operation_type: i32, operation_data: Option<&OperationData>) -> bool {
        match OperationType::try_from(operation_type) {
            // 必须走 Kafka 的操作（复杂操作）
            Ok(OperationType::Recall) => {
                // 撤回：默认全局撤回需要 Kafka
                // 可以通过 metadata 判断是否为仅自己撤回（scope == "self"）
                // 这里默认返回 true，具体判断在 handle_complex_operation 中处理
                true
            }
            Ok(OperationType::Edit) => true,    // 编辑：必须全端同步
            Ok(OperationType::Reply) => true,   // 回复：创建新消息
            Ok(OperationType::Forward) => true, // 转发：创建新消息
            Ok(OperationType::Quote) => true,   // 引用：创建新消息
            Ok(OperationType::ThreadReply) => true, // 话题：创建新消息

            // 简单操作但需要 Kafka 推送
            Ok(OperationType::Delete) => {
                // 删除：硬删除需要 Kafka，软删除不需要
                if let Some(OperationData::Delete(d)) = operation_data {
                    d.delete_type == DeleteType::Hard as i32
                } else {
                    false // 默认软删除
                }
            }
            Ok(OperationType::Read) => true, // 已读：需要推送通知（单聊通知对方，群聊更新状态）
            Ok(OperationType::ReactionAdd) | Ok(OperationType::ReactionRemove) => true, // 反应：需要同步
            Ok(OperationType::Pin) | Ok(OperationType::Unpin) => true, // 置顶：需要同步

            // 不需要 Kafka 的操作（仅用户维度）
            Ok(OperationType::Favorite) | Ok(OperationType::Unfavorite) => false, // 收藏：仅个人偏好
            Ok(OperationType::Mark) => false, // 标记：仅个人偏好

            _ => false,
        }
    }

    /// 判断操作是否需要推送通知
    ///
    /// 需要推送的情况：
    /// 1. 需要通知其他用户
    /// 2. 需要更新其他用户的状态
    pub fn requires_push_notification(operation_type: i32) -> bool {
        match OperationType::try_from(operation_type) {
            // 需要推送的操作
            Ok(OperationType::Recall) => true,  // 撤回：需要通知
            Ok(OperationType::Edit) => true,    // 编辑：需要通知
            Ok(OperationType::Delete) => true,  // 删除：需要通知（硬删除）
            Ok(OperationType::Read) => true,    // 已读：需要通知（单聊）
            Ok(OperationType::Reply) => true,   // 回复：需要通知
            Ok(OperationType::Forward) => true, // 转发：需要通知
            Ok(OperationType::ReactionAdd) | Ok(OperationType::ReactionRemove) => true, // 反应：需要通知
            Ok(OperationType::Quote) => true, // 引用：需要通知
            Ok(OperationType::ThreadReply) => true, // 话题：需要通知
            Ok(OperationType::Pin) | Ok(OperationType::Unpin) => true, // 置顶：需要通知

            // 不需要推送的操作
            Ok(OperationType::Favorite) | Ok(OperationType::Unfavorite) => false, // 收藏：仅个人
            Ok(OperationType::Mark) => false,                                     // 标记：仅个人

            _ => false,
        }
    }

    /// 判断撤回操作是否为仅自己撤回（不需要 Kafka）
    pub fn is_recall_self_only(metadata: &std::collections::HashMap<String, String>) -> bool {
        metadata.get("scope").map(|s| s.as_str()) == Some("self")
    }
}

/// 操作类别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationCategory {
    /// 简单操作（直接委托）
    Simple,
    /// 复杂操作（需要编排）
    Complex,
    /// 未知操作
    Unknown,
}
