-- 启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- 文件元数据表（常规表即可，无须 hypertable）

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

-- 创建索引
-- COMMENT: 为提高查询性能而创建的索引
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id);
CREATE INDEX IF NOT EXISTS idx_messages_timestamp ON messages(timestamp);
CREATE INDEX IF NOT EXISTS idx_messages_sender_id ON messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_messages_session_timestamp ON messages(session_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_messages_id ON messages(id);
CREATE INDEX IF NOT EXISTS idx_files_created_at ON files(created_at);
CREATE INDEX IF NOT EXISTS idx_media_assets_uploaded_at ON media_assets(uploaded_at);
CREATE INDEX IF NOT EXISTS idx_media_assets_sha256 ON media_assets(sha256);
CREATE INDEX IF NOT EXISTS idx_media_assets_status ON media_assets(status);
CREATE INDEX IF NOT EXISTS idx_media_assets_access_type ON media_assets(access_type);
CREATE INDEX IF NOT EXISTS idx_media_references_file_id ON media_references(file_id);
CREATE INDEX IF NOT EXISTS idx_media_references_namespace ON media_references(namespace);
CREATE INDEX IF NOT EXISTS idx_media_references_owner_id ON media_references(owner_id);
CREATE INDEX IF NOT EXISTS idx_media_references_created_at ON media_references(created_at);

-- 将消息表转换为 TimescaleDB 超表（Hypertable）
-- COMMENT: 按时间分区，每个分区默认 1 天，用于高效存储和查询时序消息数据
SELECT create_hypertable('messages', 'timestamp', 
    chunk_time_interval => INTERVAL '1 day',
    if_not_exists => TRUE
);

-- 配置数据保留策略（可选，保留最近 90 天的数据）
-- SELECT add_retention_policy('messages', INTERVAL '90 days');
-- SELECT add_retention_policy('files', INTERVAL '90 days');

-- 创建连续聚合视图（用于消息统计，可选）
-- COMMENT: 用于统计每小时的消息数量和唯一发送者数量
CREATE MATERIALIZED VIEW IF NOT EXISTS messages_hourly_stats
WITH (timescaledb.continuous) AS
SELECT
    time_bucket('1 hour', timestamp) AS hour,
    session_id,
    COUNT(*) AS message_count,
    COUNT(DISTINCT sender_id) AS unique_senders
FROM messages
GROUP BY hour, session_id;

-- 设置连续聚合的刷新策略
-- COMMENT: 每小时刷新一次连续聚合视图，延迟3小时以确保数据完整性
SELECT add_continuous_aggregate_policy('messages_hourly_stats',
    start_offset => INTERVAL '3 hours',
    end_offset => INTERVAL '1 hour',
    schedule_interval => INTERVAL '1 hour',
    if_not_exists => TRUE
);