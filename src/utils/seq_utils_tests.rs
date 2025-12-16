//! Seq 相关工具函数的单元测试

#[cfg(test)]
mod tests {
    use crate::utils::{
        calculate_unread_count, embed_seq_in_extra, embed_seq_in_message, extract_seq_from_extra,
        extract_seq_from_message,
    };
    use flare_proto::common::Message;
    use std::collections::HashMap;

    #[test]
    fn test_extract_seq_from_message() {
        let mut message = Message::default();

        // 测试：消息没有 seq
        assert_eq!(extract_seq_from_message(&message), None);

        // 测试：消息有有效的 seq
        message.extra.insert("seq".to_string(), "100".to_string());
        assert_eq!(extract_seq_from_message(&message), Some(100));

        // 测试：消息有无效的 seq
        message
            .extra
            .insert("seq".to_string(), "invalid".to_string());
        assert_eq!(extract_seq_from_message(&message), None);
    }

    #[test]
    fn test_embed_seq_in_message() {
        let mut message = Message::default();

        embed_seq_in_message(&mut message, 100);
        assert_eq!(message.extra.get("seq"), Some(&"100".to_string()));

        embed_seq_in_message(&mut message, 200);
        assert_eq!(message.extra.get("seq"), Some(&"200".to_string()));
    }

    #[test]
    fn test_extract_seq_from_extra() {
        let mut extra = HashMap::new();

        // 测试：extra 没有 seq
        assert_eq!(extract_seq_from_extra(&extra), None);

        // 测试：extra 有有效的 seq
        extra.insert("seq".to_string(), "100".to_string());
        assert_eq!(extract_seq_from_extra(&extra), Some(100));

        // 测试：extra 有无效的 seq
        extra.insert("seq".to_string(), "invalid".to_string());
        assert_eq!(extract_seq_from_extra(&extra), None);
    }

    #[test]
    fn test_embed_seq_in_extra() {
        let mut extra = HashMap::new();

        embed_seq_in_extra(&mut extra, 100);
        assert_eq!(extra.get("seq"), Some(&"100".to_string()));

        embed_seq_in_extra(&mut extra, 200);
        assert_eq!(extra.get("seq"), Some(&"200".to_string()));
    }

    #[test]
    fn test_calculate_unread_count() {
        // 测试：正常情况
        assert_eq!(calculate_unread_count(Some(100), 80), 20);
        assert_eq!(calculate_unread_count(Some(100), 100), 0);
        assert_eq!(calculate_unread_count(Some(100), 0), 100);

        // 测试：last_message_seq 为 None
        assert_eq!(calculate_unread_count(None, 80), 0);

        // 测试：last_read_msg_seq 大于 last_message_seq（不应该返回负数）
        assert_eq!(calculate_unread_count(Some(50), 80), 0);

        // 测试：边界情况
        assert_eq!(calculate_unread_count(Some(0), 0), 0);
        // 注意：i64::MAX 转换为 i32 会溢出，这里使用 i32::MAX 作为 last_message_seq
        assert_eq!(calculate_unread_count(Some(i32::MAX as i64), 0), i32::MAX);
    }
}
