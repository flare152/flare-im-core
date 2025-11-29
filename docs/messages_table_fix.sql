-- 修复 messages 表结构：添加缺失的字段
-- 
-- 问题：代码中使用了 seq 和 updated_at 字段，但表结构中缺少这两个字段
-- 这会导致 INSERT 语句失败

-- 1. 添加 seq 字段（序列号，用于消息排序）
ALTER TABLE messages 
ADD COLUMN IF NOT EXISTS seq BIGINT;

COMMENT ON COLUMN messages.seq IS '消息序列号（用于会话内消息排序）';

-- 2. 添加 updated_at 字段（更新时间）
-- 注意：TimescaleDB columnstore 不支持非常量默认值，需要分步处理
ALTER TABLE messages 
ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP WITH TIME ZONE;

COMMENT ON COLUMN messages.updated_at IS '消息更新时间';

-- 为现有数据设置 updated_at 值（使用 created_at 或 timestamp）
UPDATE messages 
SET updated_at = COALESCE(created_at, timestamp)
WHERE updated_at IS NULL;

-- 创建触发器函数，自动设置 updated_at（用于新插入的数据）
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- 创建触发器（在 INSERT 时自动设置 updated_at）
DROP TRIGGER IF EXISTS messages_set_updated_at ON messages;
CREATE TRIGGER messages_set_updated_at
    BEFORE INSERT ON messages
    FOR EACH ROW
    WHEN (NEW.updated_at IS NULL)
    EXECUTE FUNCTION set_updated_at();

-- 3. 为 seq 字段创建索引（用于按会话查询消息时排序）
CREATE INDEX IF NOT EXISTS idx_messages_session_seq 
ON messages (session_id, seq);

-- 4. 为 updated_at 字段创建索引（可选，用于查询最近更新的消息）
CREATE INDEX IF NOT EXISTS idx_messages_updated_at 
ON messages (updated_at DESC);

-- 5. 验证表结构
-- 执行后应该能看到 seq 和 updated_at 字段
SELECT 
    column_name, 
    data_type, 
    is_nullable,
    column_default
FROM information_schema.columns
WHERE table_name = 'messages'
ORDER BY ordinal_position;

