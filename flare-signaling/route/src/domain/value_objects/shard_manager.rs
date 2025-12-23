//! 分片管理器值对象
//!
//! 负责基于 conversation_id/user_id 的分片选择

/// 分片管理器
#[derive(Clone, Debug)]
pub struct ShardManager {
    shard_count: usize,
}

impl ShardManager {
    pub fn new(shard_count: usize) -> Self {
        Self { shard_count }
    }

    /// 选择分片
    ///
    /// # 算法
    /// 使用 FNV-1a 哈希算法（简化版）：
    /// - 输入：conversation_id 或 user_id
    /// - 输出：shard_id (0..shard_count)
    pub fn pick_shard(&self, conversation_id: Option<&str>, user_id: Option<&str>) -> usize {
        let key = conversation_id.or(user_id).unwrap_or("default");
        // 简易 FNV-1a 哈希（可替换为真正 murmur3）
        let mut hash: u64 = 1469598103934665603; // FNV offset basis
        for b in key.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
        (hash % self.shard_count.max(1) as u64) as usize
    }
}

