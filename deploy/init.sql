-- ============================================================================
-- Flare IM 数据库初始化脚本
-- ============================================================================
-- 版本: v1.0.0
-- 说明: 按模块组织数据库表结构（媒体、消息、会话、Hook引擎）
-- 数据库: PostgreSQL + TimescaleDB
-- ============================================================================

-- 启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ============================================================================
-- 1. 媒体模块 (Media Module)
-- ============================================================================
-- 职责: 媒体资产元数据存储、引用管理、去重存储

-- 媒体资产元数据表
-- COMMENT: 媒体服务核心表，存储上传的媒体文件元数据
DROP TABLE IF EXISTS media_assets CASCADE;
CREATE TABLE media_assets (
    file_id TEXT PRIMARY KEY,
    file_name TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    url TEXT NOT NULL,
    cdn_url TEXT NOT NULL,
    md5 TEXT,
    sha256 TEXT,
    metadata JSONB,
    uploaded_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    reference_count BIGINT DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'active',
    grace_expires_at TIMESTAMP WITH TIME ZONE,
    access_type TEXT NOT NULL DEFAULT 'private'
);

COMMENT ON TABLE media_assets IS '媒体资产元数据表';
COMMENT ON COLUMN media_assets.file_id IS '文件唯一标识符';
COMMENT ON COLUMN media_assets.file_name IS '文件名';
COMMENT ON COLUMN media_assets.mime_type IS 'MIME类型';
COMMENT ON COLUMN media_assets.file_size IS '文件大小（字节）';
COMMENT ON COLUMN media_assets.url IS '文件访问URL';
COMMENT ON COLUMN media_assets.cdn_url IS 'CDN访问URL';
COMMENT ON COLUMN media_assets.md5 IS 'MD5哈希值';
COMMENT ON COLUMN media_assets.sha256 IS 'SHA256哈希值';
COMMENT ON COLUMN media_assets.metadata IS '元数据（JSON格式）';
COMMENT ON COLUMN media_assets.uploaded_at IS '上传时间';
COMMENT ON COLUMN media_assets.reference_count IS '引用计数';
COMMENT ON COLUMN media_assets.status IS '文件状态（active, pending, deleted等）';
COMMENT ON COLUMN media_assets.grace_expires_at IS '宽限过期时间';
COMMENT ON COLUMN media_assets.access_type IS '文件访问类型（public, private）';

-- 媒体引用表
-- COMMENT: 媒体服务核心表，存储媒体文件的引用信息
DROP TABLE IF EXISTS media_references CASCADE;
CREATE TABLE media_references (
    reference_id TEXT PRIMARY KEY,
    file_id TEXT NOT NULL,
    namespace TEXT NOT NULL,
    owner_id TEXT NOT NULL,
    business_tag TEXT,
    metadata JSONB,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    expires_at TIMESTAMP WITH TIME ZONE,
    
    -- 外键约束
    FOREIGN KEY (file_id) REFERENCES media_assets(file_id) ON DELETE CASCADE
);

COMMENT ON TABLE media_references IS '媒体引用表';
COMMENT ON COLUMN media_references.reference_id IS '引用唯一标识符';
COMMENT ON COLUMN media_references.file_id IS '关联的文件ID';
COMMENT ON COLUMN media_references.namespace IS '命名空间';
COMMENT ON COLUMN media_references.owner_id IS '拥有者ID';
COMMENT ON COLUMN media_references.business_tag IS '业务标签';
COMMENT ON COLUMN media_references.metadata IS '引用元数据（JSON格式）';
COMMENT ON COLUMN media_references.created_at IS '创建时间';
COMMENT ON COLUMN media_references.expires_at IS '过期时间';

-- 媒体模块索引
CREATE INDEX IF NOT EXISTS idx_media_assets_uploaded_at ON media_assets(uploaded_at);
CREATE INDEX IF NOT EXISTS idx_media_assets_sha256 ON media_assets(sha256);
CREATE INDEX IF NOT EXISTS idx_media_assets_status ON media_assets(status);
CREATE INDEX IF NOT EXISTS idx_media_assets_access_type ON media_assets(access_type);
CREATE INDEX IF NOT EXISTS idx_media_references_file_id ON media_references(file_id);
CREATE INDEX IF NOT EXISTS idx_media_references_namespace ON media_references(namespace);
CREATE INDEX IF NOT EXISTS idx_media_references_owner_id ON media_references(owner_id);
CREATE INDEX IF NOT EXISTS idx_media_references_created_at ON media_references(created_at);

-- ============================================================================
-- 2. 消息模块 (Message Module)
-- ============================================================================
-- 职责: 消息持久化存储、历史消息查询、消息检索

-- 消息表（TimescaleDB Hypertable）
-- COMMENT: 消息存储核心表，使用TimescaleDB时序数据库优化，按时间分区
-- 注意：TimescaleDB要求分区列（timestamp）必须包含在主键中
DROP TABLE IF EXISTS messages CASCADE;
CREATE TABLE messages (
    id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    sender_id TEXT NOT NULL,
    receiver_ids JSONB,                    -- 接收者ID列表
    content BYTEA,                         -- 消息内容（二进制）
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    extra JSONB DEFAULT '{}',             -- 扩展字段（消息类型、业务类型等）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 消息状态和元数据（可选字段，兼容现有代码）
    message_type TEXT,                     -- 消息类型（text, image, video等）
    content_type TEXT,                     -- 内容类型（MIME类型）
    business_type TEXT,                    -- 业务类型
    status TEXT DEFAULT 'sent',            -- 消息状态（sending, sent, delivered, read, failed）
    is_recalled BOOLEAN DEFAULT FALSE,     -- 是否已撤回
    recalled_at TIMESTAMP WITH TIME ZONE,  -- 撤回时间
    is_burn_after_read BOOLEAN DEFAULT FALSE, -- 是否阅后即焚
    burn_after_seconds INTEGER,           -- 阅后即焚秒数
    visibility JSONB,                      -- 用户级别可见性 {user_id: status}
    read_by JSONB,                         -- 已阅读记录 [{user_id, read_at}]
    operations JSONB,                      -- 操作历史
    
    -- 复合主键：TimescaleDB要求分区列必须包含在主键中
    -- 使用 (timestamp, id) 顺序以优化时序查询性能
    PRIMARY KEY (timestamp, id)
);

COMMENT ON TABLE messages IS '消息存储表（TimescaleDB Hypertable）';
COMMENT ON COLUMN messages.id IS '消息唯一标识符';
COMMENT ON COLUMN messages.session_id IS '会话ID';
COMMENT ON COLUMN messages.sender_id IS '发送者ID';
COMMENT ON COLUMN messages.receiver_ids IS '接收者ID列表（JSON数组）';
COMMENT ON COLUMN messages.content IS '消息内容（二进制）';
COMMENT ON COLUMN messages.timestamp IS '消息时间戳（分区键）';
COMMENT ON COLUMN messages.extra IS '扩展字段（JSON格式）';
COMMENT ON COLUMN messages.message_type IS '消息类型（text, image, video, audio, file等）';
COMMENT ON COLUMN messages.content_type IS '内容类型（MIME类型）';
COMMENT ON COLUMN messages.business_type IS '业务类型';
COMMENT ON COLUMN messages.status IS '消息状态（sending, sent, delivered, read, failed）';
COMMENT ON COLUMN messages.is_recalled IS '是否已撤回';
COMMENT ON COLUMN messages.recalled_at IS '撤回时间';
COMMENT ON COLUMN messages.is_burn_after_read IS '是否阅后即焚';
COMMENT ON COLUMN messages.burn_after_seconds IS '阅后即焚秒数';
COMMENT ON COLUMN messages.visibility IS '用户级别可见性（JSON格式）';
COMMENT ON COLUMN messages.read_by IS '已阅读记录（JSON格式）';
COMMENT ON COLUMN messages.operations IS '操作历史（JSON格式）';

-- 消息表索引
-- 注意：主键已包含 (timestamp, id)，无需单独创建 timestamp 和 id 索引
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_messages_sender_id ON messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_messages_session_timestamp ON messages(session_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_messages_id_unique ON messages(id); -- 唯一索引，保证id全局唯一
CREATE INDEX IF NOT EXISTS idx_messages_business_type ON messages(business_type);
CREATE INDEX IF NOT EXISTS idx_messages_message_type ON messages(message_type);
CREATE INDEX IF NOT EXISTS idx_messages_status ON messages(status);
CREATE INDEX IF NOT EXISTS idx_messages_is_recalled ON messages(is_recalled);

-- 将消息表转换为 TimescaleDB 超表（Hypertable）
-- COMMENT: 按时间分区，每个分区默认 1 天，用于高效存储和查询时序消息数据
-- 注意：由于主键包含 timestamp，TimescaleDB 会自动使用主键进行分区
SELECT create_hypertable('messages', 'timestamp', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- 启用列式存储（Columnstore）用于压缩（TimescaleDB 2.x+）
-- COMMENT: TimescaleDB 2.x+ 需要先启用 columnstore 才能使用压缩策略
-- 注意：columnstore 可以提高压缩比（约 10:1），但查询性能可能略有下降
-- 对于历史数据（30天以上），压缩带来的存储节省远大于查询性能损失
-- 
-- 配置说明：
-- - enable_columnstore: 启用列式存储
-- - segmentby: 按 session_id 分段，同一会话的消息存储在一起，提高压缩效率
-- - orderby: 按 timestamp DESC, id 排序，优化时序查询性能
ALTER TABLE messages SET (
    timescaledb.enable_columnstore = true,
    timescaledb.segmentby = 'session_id',
    timescaledb.orderby = 'timestamp DESC, id'
);

-- 配置消息表列式存储策略（30天后移动到列式存储）
-- COMMENT: 自动将历史数据移动到列式存储，节省存储空间（压缩比约 10:1）
-- 列式存储的数据仍然可以正常查询，但写入性能会略有下降
-- 注意：如果 TimescaleDB 版本 < 2.x，请注释掉此策略（使用传统的 add_compression_policy）
CALL add_columnstore_policy('messages', after => INTERVAL '30 days');

-- 配置数据保留策略（可选，保留最近 90 天的数据）
-- COMMENT: 90天后的数据可以归档到对象存储或删除
-- SELECT add_retention_policy('messages', INTERVAL '90 days');

-- ============================================================================
-- 3. 会话模块 (Session Module)
-- ============================================================================
-- 职责: 会话元数据存储、参与者管理、会话状态维护

-- 会话表
-- COMMENT: 会话服务核心表，存储会话元数据和基本信息
DROP TABLE IF EXISTS sessions CASCADE;
CREATE TABLE sessions (
    session_id TEXT PRIMARY KEY,
    session_type TEXT NOT NULL,            -- 会话类型（single, group, customer等）
    business_type TEXT NOT NULL,          -- 业务类型
    display_name TEXT,                     -- 会话显示名称
    attributes JSONB,                     -- 会话属性（JSON格式）
    visibility TEXT DEFAULT 'public',      -- 可见性（public, private, hidden）
    lifecycle_state TEXT DEFAULT 'active', -- 生命周期状态（active, archived, deleted）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    metadata JSONB                        -- 扩展元数据（JSON格式）
);

COMMENT ON TABLE sessions IS '会话表';
COMMENT ON COLUMN sessions.session_id IS '会话唯一标识符';
COMMENT ON COLUMN sessions.session_type IS '会话类型（single: 单聊, group: 群聊, customer: 客服等）';
COMMENT ON COLUMN sessions.business_type IS '业务类型';
COMMENT ON COLUMN sessions.display_name IS '会话显示名称';
COMMENT ON COLUMN sessions.attributes IS '会话属性（JSON格式）';
COMMENT ON COLUMN sessions.visibility IS '可见性（public: 公开, private: 私有, hidden: 隐藏）';
COMMENT ON COLUMN sessions.lifecycle_state IS '生命周期状态（active: 活跃, archived: 归档, deleted: 已删除）';
COMMENT ON COLUMN sessions.created_at IS '创建时间';
COMMENT ON COLUMN sessions.updated_at IS '更新时间';
COMMENT ON COLUMN sessions.metadata IS '扩展元数据（JSON格式）';

-- 会话参与者表
-- COMMENT: 会话参与者关系表，存储会话成员信息
DROP TABLE IF EXISTS session_participants CASCADE;
CREATE TABLE session_participants (
    session_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    roles TEXT[],                         -- 角色列表（owner, admin, member等）
    muted BOOLEAN DEFAULT FALSE,          -- 是否静音
    pinned BOOLEAN DEFAULT FALSE,         -- 是否置顶
    attributes JSONB,                     -- 参与者属性（JSON格式）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    PRIMARY KEY (session_id, user_id),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
);

COMMENT ON TABLE session_participants IS '会话参与者表';
COMMENT ON COLUMN session_participants.session_id IS '会话ID';
COMMENT ON COLUMN session_participants.user_id IS '用户ID';
COMMENT ON COLUMN session_participants.roles IS '角色列表（owner: 拥有者, admin: 管理员, member: 成员）';
COMMENT ON COLUMN session_participants.muted IS '是否静音';
COMMENT ON COLUMN session_participants.pinned IS '是否置顶';
COMMENT ON COLUMN session_participants.attributes IS '参与者属性（JSON格式）';
COMMENT ON COLUMN session_participants.created_at IS '加入时间';
COMMENT ON COLUMN session_participants.updated_at IS '更新时间';

-- 用户同步光标表
-- COMMENT: 用户同步光标表，记录用户在各会话中的同步位置（用于多端同步）
DROP TABLE IF EXISTS user_sync_cursor CASCADE;
CREATE TABLE user_sync_cursor (
    user_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    last_synced_ts BIGINT NOT NULL,       -- 最后同步时间戳（毫秒）
    device_id TEXT,                       -- 设备ID（可选，用于设备级光标）
    version INTEGER DEFAULT 1,            -- 版本号（用于乐观锁）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    PRIMARY KEY (user_id, session_id)
);

COMMENT ON TABLE user_sync_cursor IS '用户同步光标表';
COMMENT ON COLUMN user_sync_cursor.user_id IS '用户ID';
COMMENT ON COLUMN user_sync_cursor.session_id IS '会话ID';
COMMENT ON COLUMN user_sync_cursor.last_synced_ts IS '最后同步时间戳（毫秒）';
COMMENT ON COLUMN user_sync_cursor.device_id IS '设备ID（可选，用于设备级光标）';
COMMENT ON COLUMN user_sync_cursor.version IS '版本号（用于乐观锁）';
COMMENT ON COLUMN user_sync_cursor.created_at IS '创建时间';
COMMENT ON COLUMN user_sync_cursor.updated_at IS '更新时间';

-- 会话模块索引
CREATE INDEX IF NOT EXISTS idx_sessions_business_type ON sessions(business_type, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_lifecycle_state ON sessions(lifecycle_state, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_session_type ON sessions(session_type);
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_session_participants_user_id ON session_participants(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_session_participants_session_id ON session_participants(session_id);
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_user_id ON user_sync_cursor(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_session_id ON user_sync_cursor(session_id);

-- ============================================================================
-- 4. Hook引擎模块 (Hook Engine Module)
-- ============================================================================
-- 职责: Hook配置管理、动态配置存储、多租户支持

-- Hook配置表
-- COMMENT: Hook引擎核心表，存储Hook配置信息（动态API配置，最高优先级）
DROP TABLE IF EXISTS hook_configs CASCADE;
CREATE TABLE hook_configs (
    id BIGSERIAL PRIMARY KEY,
    tenant_id TEXT,                       -- 租户ID（NULL表示全局配置）
    hook_type TEXT NOT NULL,              -- Hook类型（pre_send, post_send, delivery, recall等）
    name TEXT NOT NULL,                   -- Hook名称
    version TEXT,                         -- Hook版本
    description TEXT,                     -- Hook描述
    enabled BOOLEAN NOT NULL DEFAULT true,
    priority INTEGER NOT NULL DEFAULT 100, -- 优先级（0-1000，越小越高）
    group_name TEXT,                      -- Hook分组（validation, critical, business）
    timeout_ms BIGINT NOT NULL DEFAULT 1000,
    max_retries INTEGER NOT NULL DEFAULT 0,
    error_policy TEXT NOT NULL DEFAULT 'fail_fast', -- 错误策略（fail_fast, retry, ignore）
    require_success BOOLEAN NOT NULL DEFAULT true,
    selector_config JSONB NOT NULL DEFAULT '{}',    -- 选择器配置（JSON格式）
    transport_config JSONB NOT NULL,               -- 传输配置（JSON格式）
    metadata JSONB,                                 -- 元数据（JSON格式）
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,                                -- 创建者
    
    -- 唯一约束：同一租户下同一类型的Hook名称唯一
    UNIQUE(tenant_id, hook_type, name)
);

COMMENT ON TABLE hook_configs IS 'Hook配置表（动态API配置，最高优先级）';
COMMENT ON COLUMN hook_configs.id IS '配置ID（自增主键）';
COMMENT ON COLUMN hook_configs.tenant_id IS '租户ID（NULL表示全局配置，对所有租户生效）';
COMMENT ON COLUMN hook_configs.hook_type IS 'Hook类型（pre_send, post_send, delivery, recall, session_create, user_login等）';
COMMENT ON COLUMN hook_configs.name IS 'Hook名称（唯一标识）';
COMMENT ON COLUMN hook_configs.version IS 'Hook版本';
COMMENT ON COLUMN hook_configs.description IS 'Hook描述';
COMMENT ON COLUMN hook_configs.enabled IS '是否启用';
COMMENT ON COLUMN hook_configs.priority IS '优先级（0-1000，数字越小优先级越高）';
COMMENT ON COLUMN hook_configs.group_name IS 'Hook分组（validation: 校验组, critical: 关键组, business: 业务组）';
COMMENT ON COLUMN hook_configs.timeout_ms IS '超时时间（毫秒）';
COMMENT ON COLUMN hook_configs.max_retries IS '最大重试次数';
COMMENT ON COLUMN hook_configs.error_policy IS '错误策略（fail_fast: 快速失败, retry: 重试, ignore: 忽略）';
COMMENT ON COLUMN hook_configs.require_success IS '是否要求成功';
COMMENT ON COLUMN hook_configs.selector_config IS '选择器配置（JSON格式，包含tenants, session_types, message_types等）';
COMMENT ON COLUMN hook_configs.transport_config IS '传输配置（JSON格式，包含type, endpoint等）';
COMMENT ON COLUMN hook_configs.metadata IS '元数据（JSON格式）';
COMMENT ON COLUMN hook_configs.created_at IS '创建时间';
COMMENT ON COLUMN hook_configs.updated_at IS '更新时间';
COMMENT ON COLUMN hook_configs.created_by IS '创建者';

-- Hook配置表索引
CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant_type ON hook_configs(tenant_id, hook_type, enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_priority ON hook_configs(priority);
CREATE INDEX IF NOT EXISTS idx_hook_configs_hook_type ON hook_configs(hook_type);
CREATE INDEX IF NOT EXISTS idx_hook_configs_enabled ON hook_configs(enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_updated_at ON hook_configs(updated_at DESC);

-- ============================================================================
-- 5. 连续聚合视图（TimescaleDB Continuous Aggregates）
-- ============================================================================
-- 职责: 预聚合统计指标，提高查询性能

-- 消息每小时统计视图（TimescaleDB连续聚合）
-- COMMENT: 用于统计每小时的消息数量和唯一发送者数量
-- 注意：TimescaleDB连续聚合视图不支持 IF NOT EXISTS，需要先DROP再CREATE
DROP MATERIALIZED VIEW IF EXISTS messages_hourly_stats CASCADE;

CREATE MATERIALIZED VIEW messages_hourly_stats
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', timestamp) AS hour,
    session_id,
    COUNT(*) AS message_count,
    COUNT(DISTINCT sender_id) AS unique_senders
FROM messages
GROUP BY hour, session_id;

COMMENT ON MATERIALIZED VIEW messages_hourly_stats IS '消息每小时统计视图（TimescaleDB连续聚合）';
COMMENT ON COLUMN messages_hourly_stats.hour IS '小时时间戳';
COMMENT ON COLUMN messages_hourly_stats.session_id IS '会话ID';
COMMENT ON COLUMN messages_hourly_stats.message_count IS '消息数量';
COMMENT ON COLUMN messages_hourly_stats.unique_senders IS '唯一发送者数量';

-- 设置连续聚合的刷新策略
-- COMMENT: 每小时刷新一次连续聚合视图，延迟3小时以确保数据完整性
-- 注意：TimescaleDB 2.x+ 使用 CALL 而不是 SELECT
CALL add_continuous_aggregate_policy('messages_hourly_stats',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour',
    if_not_exists => TRUE
);

-- ============================================================================
-- 6. 触发器（自动更新时间戳）
-- ============================================================================

-- 会话表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_sessions_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_sessions_updated_at
    BEFORE UPDATE ON sessions
    FOR EACH ROW
    EXECUTE FUNCTION update_sessions_updated_at();

-- 会话参与者表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_session_participants_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_session_participants_updated_at
    BEFORE UPDATE ON session_participants
    FOR EACH ROW
    EXECUTE FUNCTION update_session_participants_updated_at();

-- 用户同步光标表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_user_sync_cursor_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_user_sync_cursor_updated_at
    BEFORE UPDATE ON user_sync_cursor
    FOR EACH ROW
    EXECUTE FUNCTION update_user_sync_cursor_updated_at();

-- Hook配置表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_hook_configs_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_hook_configs_updated_at
    BEFORE UPDATE ON hook_configs
    FOR EACH ROW
    EXECUTE FUNCTION update_hook_configs_updated_at();

-- ============================================================================
-- 初始化完成
-- ============================================================================
