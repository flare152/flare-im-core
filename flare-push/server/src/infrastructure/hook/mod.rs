//! Hook 基础设施层 - 使用 flare-hook-engine 执行 Hook

pub mod hook_envelope;
pub mod hook_executor;

pub use hook_envelope::{
    build_post_send_record, finalize_notification, prepare_message_envelope,
    prepare_notification_envelope, PushHookEnvelope,
};
pub use hook_executor::HookExecutor;

