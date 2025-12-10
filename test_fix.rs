use flare_proto::common::Message;
use prost::Message as ProstMessage;
use prost_types::Timestamp;

fn main() {
    // 模拟我们修复后的消息构造方式
    let user_id = "user1".to_string();
    let recipient_id = "user2".to_string();
    
    // 构造符合Message Orchestrator期望的session_id格式
    // 对于单聊，格式应该是 "single:sender_id:recipient_id"
    let session_id = format!("single:{}:{}", user_id, recipient_id);
    
    println!("构造的消息session_id: {}", session_id);
    
    // 检查session_id是否符合预期格式
    let session_parts: Vec<&str> = session_id.split(':').collect();
    println!("拆分后的session_parts: {:?}", session_parts);
    
    if session_parts.len() >= 3 && session_parts[0] == "single" {
        println!("✓ session_id格式正确");
        
        // 模拟Message Orchestrator中的提取逻辑
        let mut user_ids = Vec::new();
        for user_id in &session_parts[1..] {
            if *user_id != "user1" {  // sender_id
                user_ids.push(user_id.to_string());
            }
        }
        
        println!("从session_id提取的接收方user_ids: {:?}", user_ids);
        
        if !user_ids.is_empty() {
            println!("✓ 成功提取到接收方ID: {}", user_ids[0]);
        } else {
            println!("✗ 未能提取到接收方ID");
        }
    } else {
        println!("✗ session_id格式不正确");
    }
}