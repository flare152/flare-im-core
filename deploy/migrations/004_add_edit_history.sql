-- 迁移：添加编辑历史字段
-- 日期: 2025-01-XX
-- 说明: 为 messages 表添加 edit_history 字段，用于存储消息编辑历史

-- 添加 edit_history 字段（如果不存在）
ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS edit_history JSONB DEFAULT '[]'::jsonb;

COMMENT ON COLUMN messages.edit_history IS '编辑历史记录（JSONB数组），包含每次编辑的版本、内容、时间等信息';

-- 创建索引（可选，用于查询编辑历史）
CREATE INDEX IF NOT EXISTS idx_messages_edit_history 
    ON messages USING GIN (edit_history)
    WHERE edit_history IS NOT NULL AND jsonb_array_length(edit_history) > 0;

