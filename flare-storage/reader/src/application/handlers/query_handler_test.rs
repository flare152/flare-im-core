#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::queries::QueryMessagesQuery;
    use chrono::Utc;

    #[tokio::test]
    async fn test_query_messages_with_pagination() {
        // 创建一个模拟的存储仓库
        let storage = MockMessageStorage::new();
        
        // 创建查询处理器
        let query_handler = MessageStorageQueryHandler::new(Arc::new(storage));
        
        // 创建查询请求
        let query = QueryMessagesQuery {
            conversation_id: "test_session".to_string(),
            start_time: 0,
            end_time: 0,
            limit: 10,
            cursor: None,
        };
        
        // 执行查询
        let result = query_handler.handle_query_messages_with_pagination(query).await;
        
        // 验证结果
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.messages.len(), 0); // 因为是模拟存储，应该返回空列表
        assert_eq!(result.next_cursor, "");
        assert_eq!(result.has_more, false);
        assert_eq!(result.total_size, 0);
    }
}