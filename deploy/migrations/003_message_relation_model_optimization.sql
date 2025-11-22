-- ============================================================================
-- 消息关系模型优化迁移脚本
-- ============================================================================
-- 版本: v1.0.0
-- 说明: 根据《消息关系设计.md》优化数据库表结构
-- 执行顺序: 必须按顺序执行，不能跳过任何步骤
-- 注意: Flare IM Core 不提供 users 表，所有用户信息由业务系统管理
-- ============================================================================

BEGIN;

-- ============================================================================
-- 阶段1：添加新字段（向后兼容）
-- ============================================================================

-- 1.1 优化 sessions 表（Conversation）
ALTER TABLE sessions 
    ADD COLUMN IF NOT EXISTS last_message_id TEXT,
    ADD COLUMN IF NOT EXISTS last_message_seq BIGINT,
    ADD COLUMN IF NOT EXISTS is_destroyed BOOLEAN DEFAULT FALSE;

COMMENT ON COLUMN sessions.last_message_id IS '最后一条消息ID';
COMMENT ON COLUMN sessions.last_message_seq IS '最后一条消息的seq（用于未读数计算）';
COMMENT ON COLUMN sessions.is_destroyed IS '会话是否被解散（群聊）';

-- 1.2 优化 session_participants 表（ConversationUser）
ALTER TABLE session_participants
    ADD COLUMN IF NOT EXISTS last_read_msg_seq BIGINT DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_sync_msg_seq BIGINT DEFAULT 0,
    ADD COLUMN IF NOT EXISTS unread_count INTEGER DEFAULT 0,
    ADD COLUMN IF NOT EXISTS is_deleted BOOLEAN DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS mute_until TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS quit_at TIMESTAMP WITH TIME ZONE;

COMMENT ON COLUMN session_participants.last_read_msg_seq IS '已读消息的seq（用于未读数计算）';
COMMENT ON COLUMN session_participants.last_sync_msg_seq IS '多端同步游标（最后同步的seq）';
COMMENT ON COLUMN session_participants.unread_count IS '未读数（冗余字段，用于快速查询）';
COMMENT ON COLUMN session_participants.is_deleted IS '用户侧"删除会话"（软删除）';
COMMENT ON COLUMN session_participants.mute_until IS '静音截止时间（NULL表示未静音）';
COMMENT ON COLUMN session_participants.quit_at IS '退出时间（NULL表示仍在会话中）';

-- 1.3 迁移 muted 字段到 mute_until（如果存在）
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'session_participants' AND column_name = 'muted'
    ) THEN
        -- 如果 muted = true，设置 mute_until = '2099-12-31'（永久静音）
        -- 如果 muted = false，设置 mute_until = NULL
        UPDATE session_participants
        SET mute_until = CASE 
            WHEN muted = true THEN '2099-12-31'::TIMESTAMP WITH TIME ZONE
            ELSE NULL
        END
        WHERE mute_until IS NULL;
        
        -- 注意：不删除 muted 字段，保持向后兼容
        -- 如果需要删除，请先确保没有代码依赖
        -- ALTER TABLE session_participants DROP COLUMN muted;
    END IF;
END $$;

-- 1.4 优化 messages 表（Message）
ALTER TABLE messages
    ADD COLUMN IF NOT EXISTS seq BIGINT,
    ADD COLUMN IF NOT EXISTS expire_at TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS deleted_by_sender BOOLEAN DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP;

COMMENT ON COLUMN messages.seq IS '会话内递增序号（用于消息顺序和未读数计算）';
COMMENT ON COLUMN messages.expire_at IS '阅后即焚过期时间';
COMMENT ON COLUMN messages.deleted_by_sender IS '是否被发送方删除（用于审计）';
COMMENT ON COLUMN messages.updated_at IS '更新时间（用于消息编辑、撤回等）';

-- 1.5 优化 user_sync_cursor 表
ALTER TABLE user_sync_cursor
    ADD COLUMN IF NOT EXISTS last_synced_seq BIGINT DEFAULT 0;

COMMENT ON COLUMN user_sync_cursor.last_synced_seq IS '最后同步的seq（替代时间戳，更精确）';

-- ============================================================================
-- 阶段2：创建新表
-- ============================================================================

-- 2.1 创建 message_state 表（MessageState）
-- 注意：Flare IM Core 不提供 users 表，所有用户信息由业务系统管理
CREATE TABLE IF NOT EXISTS message_state (
    id BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP WITH TIME ZONE,
    is_deleted BOOLEAN DEFAULT FALSE,
    deleted_at TIMESTAMP WITH TIME ZONE,
    burn_after_read BOOLEAN DEFAULT FALSE,
    burned_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 唯一约束：同一用户对同一消息只能有一条状态记录
    UNIQUE(message_id, user_id)
);

COMMENT ON TABLE message_state IS '消息状态表（用户对消息的私有行为）';
COMMENT ON COLUMN message_state.id IS '状态记录ID（自增主键）';
COMMENT ON COLUMN message_state.message_id IS '消息ID';
COMMENT ON COLUMN message_state.user_id IS '用户ID（由业务系统提供）';
COMMENT ON COLUMN message_state.is_read IS '是否已读';
COMMENT ON COLUMN message_state.read_at IS '已读时间';
COMMENT ON COLUMN message_state.is_deleted IS '是否被用户删除（仅我删除）';
COMMENT ON COLUMN message_state.deleted_at IS '删除时间';
COMMENT ON COLUMN message_state.burn_after_read IS '是否阅后即焚';
COMMENT ON COLUMN message_state.burned_at IS '焚毁时间';
COMMENT ON COLUMN message_state.created_at IS '创建时间';
COMMENT ON COLUMN message_state.updated_at IS '更新时间';

-- ============================================================================
-- 阶段3：数据迁移
-- ============================================================================

-- 3.1 为现有消息填充 seq
-- 策略：按 session_id 分组，按 timestamp 排序，分配 seq
WITH ranked_messages AS (
    SELECT 
        id,
        session_id,
        timestamp,
        ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY timestamp, id) AS seq
    FROM messages
    WHERE seq IS NULL
)
UPDATE messages m
SET seq = rm.seq
FROM ranked_messages rm
WHERE m.id = rm.id;

-- 3.2 更新 sessions.last_message_seq
UPDATE sessions s
SET last_message_seq = (
    SELECT MAX(seq) 
    FROM messages 
    WHERE session_id = s.session_id
    AND seq IS NOT NULL
)
WHERE last_message_seq IS NULL;

-- 3.3 更新 sessions.last_message_id
UPDATE sessions s
SET last_message_id = (
    SELECT id
    FROM messages
    WHERE session_id = s.session_id
    AND seq = s.last_message_seq
    LIMIT 1
)
WHERE last_message_id IS NULL
AND last_message_seq IS NOT NULL;

-- 3.4 初始化 session_participants 的游标和未读数
UPDATE session_participants sp
SET 
    last_sync_msg_seq = COALESCE((
        SELECT MAX(seq) 
        FROM messages 
        WHERE session_id = sp.session_id
        AND seq IS NOT NULL
    ), 0),
    unread_count = COALESCE((
        SELECT COUNT(*) 
        FROM messages 
        WHERE session_id = sp.session_id 
        AND seq > COALESCE(sp.last_read_msg_seq, 0)
        AND seq IS NOT NULL
    ), 0)
WHERE last_sync_msg_seq IS NULL OR unread_count IS NULL;

-- 3.5 初始化 user_sync_cursor.last_synced_seq（从 last_synced_ts 转换）
-- 注意：由于时间戳和seq不是一一对应关系，这里使用会话的最大seq作为初始值
UPDATE user_sync_cursor usc
SET last_synced_seq = COALESCE((
    SELECT MAX(seq)
    FROM messages
    WHERE session_id = usc.session_id
    AND seq IS NOT NULL
    AND timestamp <= to_timestamp(usc.last_synced_ts / 1000)
), 0)
WHERE last_synced_seq IS NULL OR last_synced_seq = 0;

-- ============================================================================
-- 阶段4：添加约束和索引
-- ============================================================================

-- 4.1 创建索引（在数据迁移完成后）

-- sessions 表索引
CREATE INDEX IF NOT EXISTS idx_sessions_last_message_id ON sessions(last_message_id) WHERE last_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_sessions_last_message_seq ON sessions(last_message_seq) WHERE last_message_seq IS NOT NULL;

-- session_participants 表索引
CREATE INDEX IF NOT EXISTS idx_session_participants_last_read_seq ON session_participants(last_read_msg_seq);
CREATE INDEX IF NOT EXISTS idx_session_participants_last_sync_seq ON session_participants(last_sync_msg_seq);
CREATE INDEX IF NOT EXISTS idx_session_participants_unread_count ON session_participants(unread_count);
CREATE INDEX IF NOT EXISTS idx_session_participants_is_deleted ON session_participants(is_deleted) WHERE is_deleted = true;

-- messages 表索引
CREATE INDEX IF NOT EXISTS idx_messages_session_seq ON messages(session_id, seq) WHERE seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_seq ON messages(seq) WHERE seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_expire_at ON messages(expire_at) WHERE expire_at IS NOT NULL;

-- message_state 表索引
CREATE INDEX IF NOT EXISTS idx_message_state_message_id ON message_state(message_id);
CREATE INDEX IF NOT EXISTS idx_message_state_user_id ON message_state(user_id);
CREATE INDEX IF NOT EXISTS idx_message_state_is_read ON message_state(is_read) WHERE is_read = true;
CREATE INDEX IF NOT EXISTS idx_message_state_is_deleted ON message_state(is_deleted) WHERE is_deleted = true;
CREATE INDEX IF NOT EXISTS idx_message_state_burned_at ON message_state(burned_at) WHERE burned_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_message_state_user_message ON message_state(user_id, message_id);

-- user_sync_cursor 表索引
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_user_device ON user_sync_cursor(user_id, device_id, session_id) WHERE device_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_last_synced_seq ON user_sync_cursor(last_synced_seq) WHERE last_synced_seq > 0;

-- 4.2 添加 NOT NULL 约束（可选，在数据迁移完成后）
-- 注意：由于现有数据可能为NULL，这里先不添加NOT NULL约束
-- 如果需要，可以在数据迁移完成后手动添加：
-- ALTER TABLE messages ALTER COLUMN seq SET NOT NULL;
-- ALTER TABLE sessions ALTER COLUMN last_message_seq SET NOT NULL;

-- ============================================================================
-- 阶段5：创建触发器（自动更新时间戳）
-- ============================================================================

-- 5.1 message_state 表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_message_state_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trigger_message_state_updated_at ON message_state;
CREATE TRIGGER trigger_message_state_updated_at
    BEFORE UPDATE ON message_state
    FOR EACH ROW
    EXECUTE FUNCTION update_message_state_updated_at();

-- ============================================================================
-- 迁移完成
-- ============================================================================

COMMIT;

-- ============================================================================
-- 验证脚本（可选，用于验证迁移结果）
-- ============================================================================

-- 验证新字段是否添加成功
DO $$
BEGIN
    -- 检查 sessions 表新字段
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.columns 
        WHERE table_name = 'sessions' AND column_name = 'last_message_seq'
    ) THEN
        RAISE EXCEPTION 'Migration failed: sessions.last_message_seq not found';
    END IF;
    
    -- 检查 message_state 表是否存在
    IF NOT EXISTS (
        SELECT 1 FROM information_schema.tables 
        WHERE table_name = 'message_state'
    ) THEN
        RAISE EXCEPTION 'Migration failed: message_state table not found';
    END IF;
    
    RAISE NOTICE 'Migration validation passed';
END $$;

