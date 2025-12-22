-- 迁移：创建话题表
-- 日期: 2025-01-XX
-- 说明: 为话题系统添加 threads 表，支持话题管理、话题列表查询

-- 话题表（Thread）
-- COMMENT: 话题（Thread）是群组中围绕某条消息展开的讨论串
-- 参考：Discord、Slack、Telegram 的话题功能
DROP TABLE IF EXISTS threads CASCADE;
CREATE TABLE threads (
    id TEXT NOT NULL PRIMARY KEY,
    conversation_id TEXT NOT NULL,                    -- 会话ID（群组ID）
    root_message_id TEXT NOT NULL,              -- 根消息ID（话题起始消息）
    title TEXT,                                  -- 话题标题（可选，从根消息提取或用户指定）
    creator_id TEXT NOT NULL,                   -- 创建者ID
    reply_count INTEGER DEFAULT 0,               -- 回复数量（冗余字段，用于快速查询）
    last_reply_at TIMESTAMP WITH TIME ZONE,     -- 最后回复时间
    last_reply_id TEXT,                         -- 最后回复消息ID
    last_reply_user_id TEXT,                    -- 最后回复用户ID
    participant_count INTEGER DEFAULT 0,        -- 参与者数量（去重）
    is_pinned BOOLEAN DEFAULT FALSE,            -- 是否置顶
    is_locked BOOLEAN DEFAULT FALSE,            -- 是否锁定（禁止回复）
    is_archived BOOLEAN DEFAULT FALSE,           -- 是否归档
    extra JSONB DEFAULT '{}',                    -- 扩展字段
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

COMMENT ON TABLE threads IS '话题表（Thread），用于群组中的话题讨论';
COMMENT ON COLUMN threads.id IS '话题ID（通常等于 root_message_id）';
COMMENT ON COLUMN threads.conversation_id IS '会话ID（群组ID）';
COMMENT ON COLUMN threads.root_message_id IS '根消息ID（话题起始消息）';
COMMENT ON COLUMN threads.title IS '话题标题（可选，从根消息提取或用户指定）';
COMMENT ON COLUMN threads.creator_id IS '创建者ID';
COMMENT ON COLUMN threads.reply_count IS '回复数量（冗余字段，用于快速查询）';
COMMENT ON COLUMN threads.last_reply_at IS '最后回复时间';
COMMENT ON COLUMN threads.last_reply_id IS '最后回复消息ID';
COMMENT ON COLUMN threads.last_reply_user_id IS '最后回复用户ID';
COMMENT ON COLUMN threads.participant_count IS '参与者数量（去重）';
COMMENT ON COLUMN threads.is_pinned IS '是否置顶';
COMMENT ON COLUMN threads.is_locked IS '是否锁定（禁止回复）';
COMMENT ON COLUMN threads.is_archived IS '是否归档';

-- 索引
CREATE INDEX IF NOT EXISTS idx_threads_conversation_id ON threads(conversation_id);
CREATE INDEX IF NOT EXISTS idx_threads_root_message_id ON threads(root_message_id);
CREATE INDEX IF NOT EXISTS idx_threads_creator_id ON threads(creator_id);
CREATE INDEX IF NOT EXISTS idx_threads_last_reply_at ON threads(last_reply_at DESC);
CREATE INDEX IF NOT EXISTS idx_threads_is_pinned ON threads(is_pinned) WHERE is_pinned = TRUE;
CREATE INDEX IF NOT EXISTS idx_threads_is_archived ON threads(is_archived) WHERE is_archived = FALSE;

-- 话题参与者表（Thread Participants）
-- COMMENT: 记录话题的参与者，用于通知和权限控制
DROP TABLE IF EXISTS thread_participants CASCADE;
CREATE TABLE thread_participants (
    thread_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    first_participated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    last_participated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    reply_count INTEGER DEFAULT 0,              -- 该用户在此话题的回复数量
    is_muted BOOLEAN DEFAULT FALSE,             -- 是否静音（不接收通知）
    PRIMARY KEY (thread_id, user_id)
);

COMMENT ON TABLE thread_participants IS '话题参与者表，记录话题的参与者信息';
COMMENT ON COLUMN thread_participants.thread_id IS '话题ID';
COMMENT ON COLUMN thread_participants.user_id IS '用户ID';
COMMENT ON COLUMN thread_participants.first_participated_at IS '首次参与时间';
COMMENT ON COLUMN thread_participants.last_participated_at IS '最后参与时间';
COMMENT ON COLUMN thread_participants.reply_count IS '该用户在此话题的回复数量';
COMMENT ON COLUMN thread_participants.is_muted IS '是否静音（不接收通知）';

-- 索引
CREATE INDEX IF NOT EXISTS idx_thread_participants_thread_id ON thread_participants(thread_id);
CREATE INDEX IF NOT EXISTS idx_thread_participants_user_id ON thread_participants(user_id);
CREATE INDEX IF NOT EXISTS idx_thread_participants_last_participated_at ON thread_participants(last_participated_at DESC);

-- 外键约束（可选，如果启用需要确保消息存在）
-- ALTER TABLE threads ADD CONSTRAINT fk_threads_root_message 
--     FOREIGN KEY (root_message_id) REFERENCES messages(id) ON DELETE CASCADE;
-- ALTER TABLE thread_participants ADD CONSTRAINT fk_thread_participants_thread 
--     FOREIGN KEY (thread_id) REFERENCES threads(id) ON DELETE CASCADE;

