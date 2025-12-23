-- ============================================================================
-- Flare IM 数据库初始化脚本
-- ============================================================================
-- 版本: v2.0.0
-- 说明: 按模块组织数据库表结构（租户、媒体、消息、会话、话题、收藏、Hook引擎）
-- 数据库: PostgreSQL + TimescaleDB
-- 更新日期: 2025-01-XX
-- 
-- 整合内容：
-- - 001_create_admin_tables.sql: 租户表、告警规则表、告警历史表
-- - 002_create_gateway_tables.sql: Hook执行记录表优化
-- - 003_message_relation_model_optimization.sql: 消息关系模型优化（seq、message_state等）
-- - 004_add_edit_history.sql: 编辑历史字段（已整合到messages表）
-- - 005_create_threads_table.sql: 话题表和话题参与者表
-- ============================================================================

-- 启用 TimescaleDB 扩展
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- ============================================================================
-- 0. 租户和管理模块 (Tenant & Admin Module)
-- ============================================================================
-- 职责: 租户管理、Hook配置、告警规则

-- 租户表
-- COMMENT: 租户信息表，支持多租户隔离
DROP TABLE IF EXISTS tenants CASCADE;
CREATE TABLE tenants (
    tenant_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    config JSONB DEFAULT '{}'::jsonb,
    quota JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

COMMENT ON TABLE tenants IS '租户表，存储租户基本信息、配置和配额';
COMMENT ON COLUMN tenants.tenant_id IS '租户ID（主键）';
COMMENT ON COLUMN tenants.name IS '租户名称';
COMMENT ON COLUMN tenants.description IS '租户描述';
COMMENT ON COLUMN tenants.status IS '租户状态（active, suspended, deleted）';
COMMENT ON COLUMN tenants.config IS '租户配置（JSON格式）';
COMMENT ON COLUMN tenants.quota IS '租户配额（JSON格式）';

CREATE INDEX IF NOT EXISTS idx_tenants_status ON tenants(status);
CREATE INDEX IF NOT EXISTS idx_tenants_created_at ON tenants(created_at);

-- 告警规则表
DROP TABLE IF EXISTS alert_rules CASCADE;
CREATE TABLE alert_rules (
    rule_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    condition TEXT NOT NULL,
    threshold TEXT NOT NULL,
    duration_seconds INTEGER NOT NULL DEFAULT 300,
    notification_channels TEXT[],
    enabled BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

COMMENT ON TABLE alert_rules IS '告警规则表';
CREATE INDEX IF NOT EXISTS idx_alert_rules_enabled ON alert_rules(enabled);
CREATE INDEX IF NOT EXISTS idx_alert_rules_metric_name ON alert_rules(metric_name);

-- 告警历史表（TimescaleDB Hypertable）
DROP TABLE IF EXISTS alert_history CASCADE;
CREATE TABLE alert_history (
    alert_id TEXT PRIMARY KEY,
    rule_id TEXT NOT NULL,
    metric_name TEXT NOT NULL,
    current_value DOUBLE PRECISION NOT NULL,
    threshold TEXT NOT NULL,
    severity TEXT NOT NULL,
    triggered_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    resolved_at TIMESTAMP WITH TIME ZONE
);

-- SELECT create_hypertable('alert_history', 'triggered_at', 
--     chunk_time_interval => INTERVAL '1 day',
--     if_not_exists => TRUE);

CREATE INDEX IF NOT EXISTS idx_alert_history_rule_id ON alert_history(rule_id);
CREATE INDEX IF NOT EXISTS idx_alert_history_triggered_at ON alert_history(triggered_at);
CREATE INDEX IF NOT EXISTS idx_alert_history_resolved_at ON alert_history(resolved_at);

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
    server_id TEXT NOT NULL,                -- 服务端消息ID（服务端生成，全局唯一）
    conversation_id TEXT NOT NULL,
    client_msg_id TEXT,                    -- 客户端消息ID（用于去重和客户端标识）
    sender_id TEXT NOT NULL,
    content BYTEA,                         -- 消息内容（二进制）
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 消息状态和元数据（可选字段，兼容现有代码）
    message_type TEXT,                     -- 消息类型（text, image, video等）
    content_type TEXT,                     -- 内容类型（MIME类型）
    business_type TEXT,                    -- 业务类型
    status TEXT DEFAULT 'sent',            -- 消息状态（created, sent, delivered, read, failed, recalled）
    status_changed_at TIMESTAMP WITH TIME ZONE, -- 状态变更时间
    is_recalled BOOLEAN DEFAULT FALSE,     -- 是否已撤回
    recalled_at TIMESTAMP WITH TIME ZONE,  -- 撤回时间
    recall_reason TEXT,                    -- 撤回原因
    is_burn_after_read BOOLEAN DEFAULT FALSE, -- 是否阅后即焚
    burn_after_seconds INTEGER,           -- 阅后即焚秒数
    visibility JSONB,                      -- 用户级别可见性 {user_id: status}
    read_by JSONB,                         -- 已阅读记录 [{user_id, read_at}]
    operations JSONB,                      -- 操作历史
    edit_history JSONB DEFAULT '[]'::jsonb, -- 编辑历史记录 [{edit_version, content_encoded, edited_at, editor_id, reason}]
    reactions JSONB DEFAULT '[]'::jsonb,   -- 反应列表 [{emoji, user_ids, count, last_updated, created_at}]
    
    -- 消息关系模型优化字段（来自 003_message_relation_model_optimization.sql）
    seq BIGINT,                            -- 会话内递增序号（用于消息顺序和未读数计算）
    expire_at TIMESTAMP WITH TIME ZONE,    -- 阅后即焚过期时间
    deleted_by_sender BOOLEAN DEFAULT FALSE, -- 是否被发送方删除（用于审计）
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP, -- 更新时间（用于消息编辑、撤回等）
    
    -- ACK机制相关字段（优化后的ACK状态管理）
    ack_status TEXT DEFAULT 'pending',         -- ACK状态（pending, received, processed）
    ack_received BOOLEAN DEFAULT FALSE,         -- ACK是否已收到
    ack_timestamp TIMESTAMP WITH TIME ZONE,     -- ACK时间戳
    ack_user_ids JSONB,                        -- 已确认ACK的用户ID列表
    
    -- 消息操作历史记录（用于审计和追踪）
    operation_history JSONB DEFAULT '[]'::jsonb, -- 操作历史记录 [{operation_type, operator_id, timestamp, metadata}]
    
    -- 与proto文件一致的额外字段
    source TEXT,                           -- 消息来源（user, system, bot, admin）
    tenant_id TEXT,                        -- 租户ID
    attributes JSONB DEFAULT '{}'::jsonb,  -- 业务扩展字段
    extra JSONB DEFAULT '{}'::jsonb,       -- 系统扩展字段
    tags TEXT[] DEFAULT '{}',              -- 标签列表
    offline_push_info JSONB,               -- 离线推送信息
    
    -- 复合主键：TimescaleDB要求分区列必须包含在主键中
    -- 使用 (timestamp, server_id) 顺序以优化时序查询性能
    PRIMARY KEY (timestamp, server_id)
);

COMMENT ON TABLE messages IS '消息存储表（TimescaleDB Hypertable）';
COMMENT ON COLUMN messages.server_id IS '服务端消息ID（服务端生成，全局唯一）';
COMMENT ON COLUMN messages.conversation_id IS '会话ID';
COMMENT ON COLUMN messages.client_msg_id IS '客户端消息ID（客户端生成，用于去重和客户端标识）';
COMMENT ON COLUMN messages.sender_id IS '发送者ID';
COMMENT ON COLUMN messages.content IS '消息内容（二进制）';
COMMENT ON COLUMN messages.timestamp IS '消息时间戳（分区键）';
COMMENT ON COLUMN messages.extra IS '扩展字段（JSON格式）';
COMMENT ON COLUMN messages.message_type IS '消息类型（text, image, video, audio, file等）';
COMMENT ON COLUMN messages.content_type IS '内容类型（MIME类型）';
COMMENT ON COLUMN messages.business_type IS '业务类型';
COMMENT ON COLUMN messages.status IS '消息状态（created, sent, delivered, read, failed, recalled）';
COMMENT ON COLUMN messages.status_changed_at IS '状态变更时间';
COMMENT ON COLUMN messages.is_recalled IS '是否已撤回';
COMMENT ON COLUMN messages.recalled_at IS '撤回时间';
COMMENT ON COLUMN messages.recall_reason IS '撤回原因';
COMMENT ON COLUMN messages.is_burn_after_read IS '是否阅后即焚';
COMMENT ON COLUMN messages.burn_after_seconds IS '阅后即焚秒数';
COMMENT ON COLUMN messages.visibility IS '用户级别可见性（JSON格式）';
COMMENT ON COLUMN messages.read_by IS '已阅读记录（JSON格式）';
COMMENT ON COLUMN messages.operations IS '操作历史（JSON格式）';
COMMENT ON COLUMN messages.edit_history IS '编辑历史记录（JSONB数组），包含每次编辑的版本、内容、时间等信息';
COMMENT ON COLUMN messages.reactions IS '反应列表（JSONB数组），包含每个反应的emoji、用户列表、计数等信息';
COMMENT ON COLUMN messages.seq IS '会话内递增序号（用于消息顺序和未读数计算）';
COMMENT ON COLUMN messages.expire_at IS '阅后即焚过期时间';
COMMENT ON COLUMN messages.deleted_by_sender IS '是否被发送方删除（用于审计）';
COMMENT ON COLUMN messages.updated_at IS '更新时间（用于消息编辑、撤回等）';
COMMENT ON COLUMN messages.ack_status IS 'ACK状态（pending, received, processed）';
COMMENT ON COLUMN messages.ack_received IS 'ACK是否已收到';
COMMENT ON COLUMN messages.ack_timestamp IS 'ACK时间戳';
COMMENT ON COLUMN messages.ack_user_ids IS '已确认ACK的用户ID列表';
COMMENT ON COLUMN messages.operation_history IS '操作历史记录（JSONB数组），包含操作类型、操作者、时间等信息';
COMMENT ON COLUMN messages.source IS '消息来源（user, system, bot, admin）';
COMMENT ON COLUMN messages.tenant_id IS '租户ID';
COMMENT ON COLUMN messages.attributes IS '业务扩展字段';
COMMENT ON COLUMN messages.extra IS '系统扩展字段';
COMMENT ON COLUMN messages.tags IS '标签列表';
COMMENT ON COLUMN messages.offline_push_info IS '离线推送信息';

-- 消息表索引
-- 注意：主键已包含 (timestamp, server_id)，无需单独创建 timestamp 和 server_id 索引
CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON messages(conversation_id);
CREATE INDEX IF NOT EXISTS idx_messages_sender_id ON messages(sender_id);
CREATE INDEX IF NOT EXISTS idx_messages_conversation_timestamp ON messages(conversation_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_messages_server_id_unique ON messages(server_id); -- 唯一索引，保证server_id全局唯一
CREATE INDEX IF NOT EXISTS idx_messages_client_msg_id ON messages(client_msg_id) WHERE client_msg_id IS NOT NULL; -- 客户端消息ID索引（用于去重查询）
CREATE INDEX IF NOT EXISTS idx_messages_sender_client_msg_id ON messages(sender_id, client_msg_id) WHERE client_msg_id IS NOT NULL; -- 发送者+客户端消息ID复合索引（用于幂等性检查）
CREATE INDEX IF NOT EXISTS idx_messages_business_type ON messages(business_type);
CREATE INDEX IF NOT EXISTS idx_messages_message_type ON messages(message_type);
CREATE INDEX IF NOT EXISTS idx_messages_status ON messages(status);
CREATE INDEX IF NOT EXISTS idx_messages_status_changed_at ON messages(status_changed_at) WHERE status_changed_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_is_recalled ON messages(is_recalled);
CREATE INDEX IF NOT EXISTS idx_messages_conversation_seq ON messages(conversation_id, seq) WHERE seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_seq ON messages(seq) WHERE seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_expire_at ON messages(expire_at) WHERE expire_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_ack_status ON messages(ack_status) WHERE ack_status IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_ack_received ON messages(ack_received) WHERE ack_received = true;
CREATE INDEX IF NOT EXISTS idx_messages_ack_timestamp ON messages(ack_timestamp) WHERE ack_timestamp IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_ack_user_ids ON messages USING GIN(ack_user_ids) WHERE ack_user_ids IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_source ON messages(source) WHERE source IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_tenant_id ON messages(tenant_id) WHERE tenant_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_messages_tags ON messages USING GIN(tags) WHERE tags IS NOT NULL AND tags != '{}';

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
-- - segmentby: 按 conversation_id 分段，同一会话的消息存储在一起，提高压缩效率
-- - orderby: 按 timestamp DESC, server_id 排序，优化时序查询性能
ALTER TABLE messages SET (
    timescaledb.enable_columnstore = true,
    timescaledb.segmentby = 'conversation_id',
    timescaledb.orderby = 'timestamp DESC, server_id'
);

-- 配置消息表列式存储策略（30天后移动到列式存储）
-- COMMENT: 自动将历史数据移动到列式存储，节省存储空间（压缩比约 10:1）
-- 列式存储的数据仍然可以正常查询，但写入性能会略有下降
-- 注意：如果 TimescaleDB 版本 < 2.x，请注释掉此策略（使用传统的 add_compression_policy）
CALL add_columnstore_policy('messages', after => INTERVAL '30 days');

-- 配置数据保留策略（可选，保留最近 90 天的数据）
-- COMMENT: 90天后的数据可以归档到对象存储或删除
-- SELECT add_retention_policy('messages', INTERVAL '90 days');

-- ACK归档记录表（AckArchiveRecords）
-- COMMENT: ACK归档记录表，用于审计和分析的ACK日志归档
DROP TABLE IF EXISTS ack_archive_records CASCADE;
CREATE TABLE ack_archive_records (
    id BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    ack_type TEXT NOT NULL,
    ack_status TEXT NOT NULL,
    timestamp BIGINT NOT NULL,
    importance_level SMALLINT DEFAULT 1 CHECK (importance_level BETWEEN 1 AND 3),
    metadata JSONB,
    archived_at BIGINT NOT NULL DEFAULT EXTRACT(EPOCH FROM NOW())
);

COMMENT ON TABLE ack_archive_records IS 'ACK归档记录表（用于审计和分析的ACK日志归档）';
COMMENT ON COLUMN ack_archive_records.id IS '记录ID（自增主键）';
COMMENT ON COLUMN ack_archive_records.message_id IS '消息ID';
COMMENT ON COLUMN ack_archive_records.user_id IS '用户ID';
COMMENT ON COLUMN ack_archive_records.ack_type IS 'ACK类型';
COMMENT ON COLUMN ack_archive_records.ack_status IS 'ACK状态';
COMMENT ON COLUMN ack_archive_records.timestamp IS '时间戳';
COMMENT ON COLUMN ack_archive_records.importance_level IS '重要性等级：1-低，2-中，3-高';
COMMENT ON COLUMN ack_archive_records.metadata IS '元数据';
COMMENT ON COLUMN ack_archive_records.archived_at IS '归档时间';

-- ACK归档记录表索引
CREATE INDEX IF NOT EXISTS idx_ack_archive_message_id ON ack_archive_records (message_id);
CREATE INDEX IF NOT EXISTS idx_ack_archive_user_id ON ack_archive_records (user_id);
CREATE INDEX IF NOT EXISTS idx_ack_archive_timestamp_desc ON ack_archive_records (timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_ack_archive_importance_level ON ack_archive_records (importance_level);
CREATE INDEX IF NOT EXISTS idx_ack_archive_message_user_type ON ack_archive_records (message_id, user_id, ack_type);

-- 消息可靠性保障表（MessageReliability）
-- COMMENT: 消息可靠性保障表，用于跟踪消息的发送、确认和重试状态
DROP TABLE IF EXISTS message_reliability CASCADE;
CREATE TABLE message_reliability (
    id BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL,               -- 消息ID
    conversation_id TEXT NOT NULL,               -- 会话ID
    sender_id TEXT NOT NULL,                -- 发送者ID
    recipient_ids JSONB,                    -- 接收者ID列表
    send_attempts INTEGER DEFAULT 0,        -- 发送尝试次数
    max_send_attempts INTEGER DEFAULT 3,    -- 最大发送尝试次数
    last_send_attempt TIMESTAMP WITH TIME ZONE, -- 最后发送尝试时间
    delivery_status TEXT DEFAULT 'pending', -- 投递状态（pending, delivered, failed）
    confirmation_status TEXT DEFAULT 'pending', -- 确认状态（pending, confirmed, failed）
    retry_count INTEGER DEFAULT 0,          -- 重试次数
    max_retry_count INTEGER DEFAULT 5,      -- 最大重试次数
    next_retry_at TIMESTAMP WITH TIME ZONE, -- 下次重试时间
    error_code TEXT,                        -- 错误码
    error_message TEXT,                     -- 错误信息
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 唯一约束：每个消息只能有一条记录
    UNIQUE(message_id)
);

-- 系统监控指标表（SystemMetrics）
-- COMMENT: 系统监控指标表，用于收集和存储系统性能指标
DROP TABLE IF EXISTS system_metrics CASCADE;
CREATE TABLE system_metrics (
    id BIGSERIAL PRIMARY KEY,
    metric_name TEXT NOT NULL,              -- 指标名称
    metric_value DOUBLE PRECISION,          -- 指标值
    metric_unit TEXT,                       -- 指标单位
    metric_type TEXT,                       -- 指标类型（counter, gauge, histogram, summary）
    service_name TEXT,                      -- 服务名称
    node_id TEXT,                           -- 节点ID
    tenant_id TEXT,                         -- 租户ID
    tags JSONB,                             -- 标签
    recorded_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP, -- 记录时间
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

COMMENT ON TABLE message_reliability IS '消息可靠性保障表（用于跟踪消息的发送、确认和重试状态）';
COMMENT ON COLUMN message_reliability.id IS '记录ID（自增主键）';
COMMENT ON COLUMN message_reliability.message_id IS '消息ID';
COMMENT ON COLUMN message_reliability.conversation_id IS '会话ID';
COMMENT ON COLUMN message_reliability.sender_id IS '发送者ID';
COMMENT ON COLUMN message_reliability.recipient_ids IS '接收者ID列表';
COMMENT ON COLUMN message_reliability.send_attempts IS '发送尝试次数';
COMMENT ON COLUMN message_reliability.max_send_attempts IS '最大发送尝试次数';
COMMENT ON COLUMN message_reliability.last_send_attempt IS '最后发送尝试时间';
COMMENT ON COLUMN message_reliability.delivery_status IS '投递状态（pending, delivered, failed）';
COMMENT ON COLUMN message_reliability.confirmation_status IS '确认状态（pending, confirmed, failed）';
COMMENT ON COLUMN message_reliability.retry_count IS '重试次数';
COMMENT ON COLUMN message_reliability.max_retry_count IS '最大重试次数';
COMMENT ON COLUMN message_reliability.next_retry_at IS '下次重试时间';
COMMENT ON COLUMN message_reliability.error_code IS '错误码';
COMMENT ON COLUMN message_reliability.error_message IS '错误信息';
COMMENT ON COLUMN message_reliability.created_at IS '创建时间';
COMMENT ON COLUMN message_reliability.updated_at IS '更新时间';

COMMENT ON TABLE system_metrics IS '系统监控指标表（用于收集和存储系统性能指标）';
COMMENT ON COLUMN system_metrics.id IS '记录ID（自增主键）';
COMMENT ON COLUMN system_metrics.metric_name IS '指标名称';
COMMENT ON COLUMN system_metrics.metric_value IS '指标值';
COMMENT ON COLUMN system_metrics.metric_unit IS '指标单位';
COMMENT ON COLUMN system_metrics.metric_type IS '指标类型（counter, gauge, histogram, summary）';
COMMENT ON COLUMN system_metrics.service_name IS '服务名称';
COMMENT ON COLUMN system_metrics.node_id IS '节点ID';
COMMENT ON COLUMN system_metrics.tenant_id IS '租户ID';
COMMENT ON COLUMN system_metrics.tags IS '标签';
COMMENT ON COLUMN system_metrics.recorded_at IS '记录时间';
COMMENT ON COLUMN system_metrics.created_at IS '创建时间';

-- 消息可靠性保障表索引
CREATE INDEX IF NOT EXISTS idx_message_reliability_message_id ON message_reliability(message_id);
CREATE INDEX IF NOT EXISTS idx_message_reliability_conversation_id ON message_reliability(conversation_id);
CREATE INDEX IF NOT EXISTS idx_message_reliability_sender_id ON message_reliability(sender_id);
CREATE INDEX IF NOT EXISTS idx_message_reliability_delivery_status ON message_reliability(delivery_status);
CREATE INDEX IF NOT EXISTS idx_message_reliability_confirmation_status ON message_reliability(confirmation_status);
CREATE INDEX IF NOT EXISTS idx_message_reliability_next_retry_at ON message_reliability(next_retry_at) WHERE next_retry_at IS NOT NULL;

-- 系统监控指标表索引
CREATE INDEX IF NOT EXISTS idx_system_metrics_metric_name ON system_metrics(metric_name);
CREATE INDEX IF NOT EXISTS idx_system_metrics_service_name ON system_metrics(service_name);
CREATE INDEX IF NOT EXISTS idx_system_metrics_node_id ON system_metrics(node_id);
CREATE INDEX IF NOT EXISTS idx_system_metrics_tenant_id ON system_metrics(tenant_id);
CREATE INDEX IF NOT EXISTS idx_system_metrics_recorded_at ON system_metrics(recorded_at);
CREATE INDEX IF NOT EXISTS idx_system_metrics_tags ON system_metrics USING GIN(tags) WHERE tags IS NOT NULL;

-- 消息可靠性保障表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_message_reliability_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_message_reliability_updated_at
    BEFORE UPDATE ON message_reliability
    FOR EACH ROW
    EXECUTE FUNCTION update_message_reliability_updated_at();

-- 系统监控指标表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_system_metrics_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.created_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_system_metrics_updated_at
    BEFORE INSERT ON system_metrics
    FOR EACH ROW
    EXECUTE FUNCTION update_system_metrics_updated_at();

-- ============================================================================
-- 3. 会话模块 (Conversation Module)
-- ============================================================================
-- 职责: 会话元数据存储、参与者管理、会话状态维护

-- 会话表
-- COMMENT: 会话服务核心表，存储会话元数据和基本信息
DROP TABLE IF EXISTS conversations CASCADE;
CREATE TABLE conversations (
    conversation_id TEXT PRIMARY KEY,
    conversation_type TEXT NOT NULL,            -- 会话类型（single, group, channel等）
    business_type TEXT NOT NULL,          -- 业务类型
    display_name TEXT,                     -- 会话显示名称
    attributes JSONB,                     -- 会话属性（JSON格式）
    visibility TEXT DEFAULT 'public',      -- 可见性（public, private, hidden）
    lifecycle_state TEXT DEFAULT 'active', -- 生命周期状态（active, archived, deleted）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    metadata JSONB,                        -- 扩展元数据（JSON格式）
    
    -- 消息关系模型优化字段（来自 003_message_relation_model_optimization.sql）
    last_message_id TEXT,                  -- 最后一条消息ID
    last_message_seq BIGINT,               -- 最后一条消息的seq（用于未读数计算）
    is_destroyed BOOLEAN DEFAULT FALSE,    -- 会话是否被解散（群聊）
    
    -- 与proto文件一致的额外字段
    description TEXT,                      -- 会话描述
    avatar_url TEXT,                       -- 会话头像URL
    owner_id TEXT,                         -- 会话拥有者ID
    max_members INTEGER,                   -- 最大成员数
    is_public BOOLEAN DEFAULT FALSE,       -- 是否公开会话
    join_approval_required BOOLEAN DEFAULT FALSE, -- 加入是否需要审批
    enable_history_browsing BOOLEAN DEFAULT TRUE, -- 是否允许浏览历史消息
    enable_message_reactions BOOLEAN DEFAULT TRUE, -- 是否允许消息反应
    enable_message_edit BOOLEAN DEFAULT TRUE, -- 是否允许编辑消息
    enable_message_delete BOOLEAN DEFAULT TRUE, -- 是否允许删除消息
    message_ttl_seconds INTEGER,           -- 消息生存时间（秒）
    notification_level TEXT DEFAULT 'all', -- 通知级别（all, mention, none）
    tags TEXT[] DEFAULT '{}',              -- 标签列表
    custom_data JSONB DEFAULT '{}'::jsonb  -- 自定义数据
);

COMMENT ON TABLE conversations IS '会话表';
COMMENT ON COLUMN conversations.conversation_id IS '会话唯一标识符';
COMMENT ON COLUMN conversations.conversation_type IS '会话类型（single: 单聊, group: 群聊, channel: 频道）';
COMMENT ON COLUMN conversations.business_type IS '业务类型';
COMMENT ON COLUMN conversations.display_name IS '会话显示名称';
COMMENT ON COLUMN conversations.attributes IS '会话属性（JSON格式）';
COMMENT ON COLUMN conversations.visibility IS '可见性（public: 公开, private: 私有, hidden: 隐藏）';
COMMENT ON COLUMN conversations.lifecycle_state IS '生命周期状态（active: 活跃, archived: 归档, deleted: 已删除）';
COMMENT ON COLUMN conversations.created_at IS '创建时间';
COMMENT ON COLUMN conversations.updated_at IS '更新时间';
COMMENT ON COLUMN conversations.metadata IS '扩展元数据（JSON格式）';
COMMENT ON COLUMN conversations.last_message_id IS '最后一条消息ID';
COMMENT ON COLUMN conversations.last_message_seq IS '最后一条消息的seq（用于未读数计算）';
COMMENT ON COLUMN conversations.is_destroyed IS '会话是否被解散（群聊）';
COMMENT ON COLUMN conversations.description IS '会话描述';
COMMENT ON COLUMN conversations.avatar_url IS '会话头像URL';
COMMENT ON COLUMN conversations.owner_id IS '会话拥有者ID';
COMMENT ON COLUMN conversations.max_members IS '最大成员数';
COMMENT ON COLUMN conversations.is_public IS '是否公开会话';
COMMENT ON COLUMN conversations.join_approval_required IS '加入是否需要审批';
COMMENT ON COLUMN conversations.enable_history_browsing IS '是否允许浏览历史消息';
COMMENT ON COLUMN conversations.enable_message_reactions IS '是否允许消息反应';
COMMENT ON COLUMN conversations.enable_message_edit IS '是否允许编辑消息';
COMMENT ON COLUMN conversations.enable_message_delete IS '是否允许删除消息';
COMMENT ON COLUMN conversations.message_ttl_seconds IS '消息生存时间（秒）';
COMMENT ON COLUMN conversations.notification_level IS '通知级别（all, mention, none）';
COMMENT ON COLUMN conversations.tags IS '标签列表';
COMMENT ON COLUMN conversations.custom_data IS '自定义数据';

-- 会话参与者表
-- COMMENT: 会话参与者关系表，存储会话成员信息
DROP TABLE IF EXISTS conversation_participants CASCADE;
CREATE TABLE conversation_participants (
    conversation_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    roles TEXT[],                         -- 角色列表（owner: 拥有者, admin: 管理员, member: 成员, guest: 访客, observer: 观察者）
    muted BOOLEAN DEFAULT FALSE,          -- 是否静音（向后兼容，建议使用 mute_until）
    pinned BOOLEAN DEFAULT FALSE,         -- 是否置顶
    attributes JSONB,                     -- 参与者属性（JSON格式）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 消息关系模型优化字段（来自 003_message_relation_model_optimization.sql）
    last_read_msg_seq BIGINT DEFAULT 0,   -- 已读消息的seq（用于未读数计算）
    last_sync_msg_seq BIGINT DEFAULT 0,   -- 多端同步游标（最后同步的seq）
    unread_count INTEGER DEFAULT 0,        -- 未读数（冗余字段，用于快速查询）
    is_deleted BOOLEAN DEFAULT FALSE,      -- 用户侧"删除会话"（软删除）
    mute_until TIMESTAMP WITH TIME ZONE,   -- 静音截止时间（NULL表示未静音）
    quit_at TIMESTAMP WITH TIME ZONE,      -- 退出时间（NULL表示仍在会话中）
    
    PRIMARY KEY (conversation_id, user_id),
    FOREIGN KEY (conversation_id) REFERENCES conversations(conversation_id) ON DELETE CASCADE
);

COMMENT ON TABLE conversation_participants IS '会话参与者表';
COMMENT ON COLUMN conversation_participants.conversation_id IS '会话ID';
COMMENT ON COLUMN conversation_participants.user_id IS '用户ID';
COMMENT ON COLUMN conversation_participants.roles IS '角色列表（owner: 拥有者, admin: 管理员, member: 成员, guest: 访客, observer: 观察者）';
COMMENT ON COLUMN conversation_participants.muted IS '是否静音';
COMMENT ON COLUMN conversation_participants.pinned IS '是否置顶';
COMMENT ON COLUMN conversation_participants.attributes IS '参与者属性（JSON格式）';
COMMENT ON COLUMN conversation_participants.created_at IS '加入时间';
COMMENT ON COLUMN conversation_participants.updated_at IS '更新时间';
COMMENT ON COLUMN conversation_participants.last_read_msg_seq IS '已读消息的seq（用于未读数计算）';
COMMENT ON COLUMN conversation_participants.last_sync_msg_seq IS '多端同步游标（最后同步的seq）';
COMMENT ON COLUMN conversation_participants.unread_count IS '未读数（冗余字段，用于快速查询）';
COMMENT ON COLUMN conversation_participants.is_deleted IS '用户侧"删除会话"（软删除）';
COMMENT ON COLUMN conversation_participants.mute_until IS '静音截止时间（NULL表示未静音）';
COMMENT ON COLUMN conversation_participants.quit_at IS '退出时间（NULL表示仍在会话中）';

-- 用户同步光标表
-- COMMENT: 用户同步光标表，记录用户在各会话中的同步位置（用于多端同步）
DROP TABLE IF EXISTS user_sync_cursor CASCADE;
CREATE TABLE user_sync_cursor (
    user_id TEXT NOT NULL,
    conversation_id TEXT NOT NULL,
    last_synced_ts BIGINT NOT NULL,       -- 最后同步时间戳（毫秒）
    device_id TEXT,                       -- 设备ID（可选，用于设备级光标）
    version INTEGER DEFAULT 1,            -- 版本号（用于乐观锁）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 消息关系模型优化字段（来自 003_message_relation_model_optimization.sql）
    last_synced_seq BIGINT DEFAULT 0,     -- 最后同步的seq（替代时间戳，更精确）
    
    PRIMARY KEY (user_id, conversation_id)
);

COMMENT ON TABLE user_sync_cursor IS '用户同步光标表';
COMMENT ON COLUMN user_sync_cursor.user_id IS '用户ID';
COMMENT ON COLUMN user_sync_cursor.conversation_id IS '会话ID';
COMMENT ON COLUMN user_sync_cursor.last_synced_ts IS '最后同步时间戳（毫秒）';
COMMENT ON COLUMN user_sync_cursor.device_id IS '设备ID（可选，用于设备级光标）';
COMMENT ON COLUMN user_sync_cursor.version IS '版本号（用于乐观锁）';
COMMENT ON COLUMN user_sync_cursor.created_at IS '创建时间';
COMMENT ON COLUMN user_sync_cursor.updated_at IS '更新时间';
COMMENT ON COLUMN user_sync_cursor.last_synced_seq IS '最后同步的seq（替代时间戳，更精确）';

-- 消息状态表（MessageState）
-- COMMENT: 消息状态表，记录用户对消息的私有行为（已读、删除、阅后即焚等）
-- 注意：Flare IM Core 不提供 users 表，所有用户信息由业务系统管理
DROP TABLE IF EXISTS message_state CASCADE;
CREATE TABLE message_state (
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

-- 消息操作历史记录表（MessageOperationHistory）
-- COMMENT: 消息操作历史记录表，记录对消息的所有操作（审计和追踪）
DROP TABLE IF EXISTS message_operation_history CASCADE;
CREATE TABLE message_operation_history (
    id BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL,
    operation_type TEXT NOT NULL,           -- 操作类型（recall, edit, delete, read, forward等）
    operator_id TEXT NOT NULL,              -- 操作者ID
    target_user_id TEXT,                    -- 目标用户ID（可选，用于定向操作）
    operation_data JSONB,                   -- 操作数据（根据操作类型不同而不同）
    timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    metadata JSONB DEFAULT '{}'::jsonb      -- 元数据（扩展字段）
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

COMMENT ON TABLE message_operation_history IS '消息操作历史记录表（记录对消息的所有操作）';
COMMENT ON COLUMN message_operation_history.id IS '操作记录ID（自增主键）';
COMMENT ON COLUMN message_operation_history.message_id IS '消息ID';
COMMENT ON COLUMN message_operation_history.operation_type IS '操作类型（recall, edit, delete, read, forward等）';
COMMENT ON COLUMN message_operation_history.operator_id IS '操作者ID';
COMMENT ON COLUMN message_operation_history.target_user_id IS '目标用户ID（可选，用于定向操作）';
COMMENT ON COLUMN message_operation_history.operation_data IS '操作数据（根据操作类型不同而不同）';
COMMENT ON COLUMN message_operation_history.timestamp IS '操作时间戳';
COMMENT ON COLUMN message_operation_history.created_at IS '创建时间';
COMMENT ON COLUMN message_operation_history.metadata IS '元数据（扩展字段）';

-- 会话模块索引
CREATE INDEX IF NOT EXISTS idx_conversations_business_type ON conversations(business_type, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversations_lifecycle_state ON conversations(lifecycle_state, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversations_conversation_type ON conversations(conversation_type);
CREATE INDEX IF NOT EXISTS idx_conversations_updated_at ON conversations(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversations_owner_id ON conversations(owner_id) WHERE owner_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conversations_is_public ON conversations(is_public) WHERE is_public = true;
CREATE INDEX IF NOT EXISTS idx_conversations_notification_level ON conversations(notification_level);
CREATE INDEX IF NOT EXISTS idx_conversations_tags ON conversations USING GIN(tags) WHERE tags IS NOT NULL AND tags != '{}';
CREATE INDEX IF NOT EXISTS idx_conversation_participants_user_id ON conversation_participants(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_conversation_id ON conversation_participants(conversation_id);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_last_read_seq ON conversation_participants(last_read_msg_seq);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_last_sync_seq ON conversation_participants(last_sync_msg_seq);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_unread_count ON conversation_participants(unread_count);
CREATE INDEX IF NOT EXISTS idx_conversation_participants_is_deleted ON conversation_participants(is_deleted) WHERE is_deleted = true;
CREATE INDEX IF NOT EXISTS idx_conversations_last_message_id ON conversations(last_message_id) WHERE last_message_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_conversations_last_message_seq ON conversations(last_message_seq) WHERE last_message_seq IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_user_id ON user_sync_cursor(user_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_conversation_id ON user_sync_cursor(conversation_id);
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_user_device ON user_sync_cursor(user_id, device_id, conversation_id) WHERE device_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_user_sync_cursor_last_synced_seq ON user_sync_cursor(last_synced_seq) WHERE last_synced_seq > 0;
CREATE INDEX IF NOT EXISTS idx_message_state_message_id ON message_state(message_id);
CREATE INDEX IF NOT EXISTS idx_message_state_user_id ON message_state(user_id);
CREATE INDEX IF NOT EXISTS idx_message_state_is_read ON message_state(is_read) WHERE is_read = true;
CREATE INDEX IF NOT EXISTS idx_message_state_is_deleted ON message_state(is_deleted) WHERE is_deleted = true;
CREATE INDEX IF NOT EXISTS idx_message_state_burned_at ON message_state(burned_at) WHERE burned_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_message_state_user_message ON message_state(user_id, message_id);
CREATE INDEX IF NOT EXISTS idx_message_operation_history_message_id ON message_operation_history(message_id);
CREATE INDEX IF NOT EXISTS idx_message_operation_history_operation_type ON message_operation_history(operation_type);
CREATE INDEX IF NOT EXISTS idx_message_operation_history_operator_id ON message_operation_history(operator_id);
CREATE INDEX IF NOT EXISTS idx_message_operation_history_timestamp ON message_operation_history(timestamp DESC);

-- 消息ACK记录表（MessageAckRecords）
-- COMMENT: 消息ACK记录表，记录所有ACK相关信息
DROP TABLE IF EXISTS message_ack_records CASCADE;
CREATE TABLE message_ack_records (
    id BIGSERIAL PRIMARY KEY,
    message_id TEXT NOT NULL,               -- 消息ID
    user_id TEXT NOT NULL,                  -- 用户ID
    ack_type TEXT NOT NULL,                 -- ACK类型（client, push, storage）
    ack_status TEXT NOT NULL,               -- ACK状态（received, processed, failed）
    ack_timestamp TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP, -- ACK时间戳
    device_id TEXT,                         -- 设备ID（可选）
    client_msg_id TEXT,                     -- 客户端消息ID（可选）
    error_code TEXT,                        -- 错误码（可选）
    error_message TEXT,                     -- 错误信息（可选）
    metadata JSONB DEFAULT '{}'::jsonb,     -- 元数据（扩展字段）
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 唯一约束：同一用户对同一消息的同一类型ACK只能有一条记录
    UNIQUE(message_id, user_id, ack_type)
);

COMMENT ON TABLE message_ack_records IS '消息ACK记录表（记录所有ACK相关信息）';
COMMENT ON COLUMN message_ack_records.id IS 'ACK记录ID（自增主键）';
COMMENT ON COLUMN message_ack_records.message_id IS '消息ID';
COMMENT ON COLUMN message_ack_records.user_id IS '用户ID';
COMMENT ON COLUMN message_ack_records.ack_type IS 'ACK类型（client, push, storage）';
COMMENT ON COLUMN message_ack_records.ack_status IS 'ACK状态（received, processed, failed）';
COMMENT ON COLUMN message_ack_records.ack_timestamp IS 'ACK时间戳';
COMMENT ON COLUMN message_ack_records.device_id IS '设备ID（可选）';
COMMENT ON COLUMN message_ack_records.client_msg_id IS '客户端消息ID（可选）';
COMMENT ON COLUMN message_ack_records.error_code IS '错误码（可选）';
COMMENT ON COLUMN message_ack_records.error_message IS '错误信息（可选）';
COMMENT ON COLUMN message_ack_records.metadata IS '元数据（扩展字段）';
COMMENT ON COLUMN message_ack_records.created_at IS '创建时间';
COMMENT ON COLUMN message_ack_records.updated_at IS '更新时间';

-- 消息ACK记录表索引
CREATE INDEX IF NOT EXISTS idx_message_ack_records_message_id ON message_ack_records(message_id);
CREATE INDEX IF NOT EXISTS idx_message_ack_records_user_id ON message_ack_records(user_id);
CREATE INDEX IF NOT EXISTS idx_message_ack_records_ack_type ON message_ack_records(ack_type);
CREATE INDEX IF NOT EXISTS idx_message_ack_records_ack_status ON message_ack_records(ack_status);
CREATE INDEX IF NOT EXISTS idx_message_ack_records_ack_timestamp ON message_ack_records(ack_timestamp DESC);
CREATE INDEX IF NOT EXISTS idx_message_ack_records_user_message ON message_ack_records(user_id, message_id);

-- 消息ACK记录表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_message_ack_records_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_message_ack_records_updated_at
    BEFORE UPDATE ON message_ack_records
    FOR EACH ROW
    EXECUTE FUNCTION update_message_ack_records_updated_at();

-- ============================================================================
-- 4. 话题模块 (Thread Module)
-- ============================================================================
-- 职责: 话题管理、话题参与者管理、话题通知

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

-- 话题模块索引
CREATE INDEX IF NOT EXISTS idx_threads_conversation_id ON threads(conversation_id);
CREATE INDEX IF NOT EXISTS idx_threads_root_message_id ON threads(root_message_id);
CREATE INDEX IF NOT EXISTS idx_threads_creator_id ON threads(creator_id);
CREATE INDEX IF NOT EXISTS idx_threads_last_reply_at ON threads(last_reply_at DESC);
CREATE INDEX IF NOT EXISTS idx_threads_is_pinned ON threads(is_pinned) WHERE is_pinned = TRUE;
CREATE INDEX IF NOT EXISTS idx_threads_is_archived ON threads(is_archived) WHERE is_archived = FALSE;
CREATE INDEX IF NOT EXISTS idx_thread_participants_thread_id ON thread_participants(thread_id);
CREATE INDEX IF NOT EXISTS idx_thread_participants_user_id ON thread_participants(user_id);
CREATE INDEX IF NOT EXISTS idx_thread_participants_last_participated_at ON thread_participants(last_participated_at DESC);

-- ============================================================================
-- 5. 收藏模块 (Favorite Module)
-- ============================================================================
-- 职责: 用户消息收藏管理

-- 收藏表（Favorites）
-- COMMENT: 用户收藏的消息，支持标签和备注
DROP TABLE IF EXISTS favorites CASCADE;
CREATE TABLE favorites (
    id TEXT NOT NULL PRIMARY KEY,
    user_id TEXT NOT NULL,                      -- 用户ID
    message_id TEXT NOT NULL,                   -- 消息ID
    conversation_id TEXT NOT NULL,                   -- 会话ID（冗余，用于快速查询）
    tags TEXT[] DEFAULT '{}',                    -- 收藏标签（数组）
    note TEXT,                                  -- 收藏备注
    favorited_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,  -- 收藏时间
    extra JSONB DEFAULT '{}',                   -- 扩展字段
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    
    -- 唯一约束：一个用户只能收藏同一条消息一次
    UNIQUE(user_id, message_id)
);

COMMENT ON TABLE favorites IS '用户收藏表，记录用户收藏的消息';
COMMENT ON COLUMN favorites.id IS '收藏记录ID（UUID）';
COMMENT ON COLUMN favorites.user_id IS '用户ID';
COMMENT ON COLUMN favorites.message_id IS '消息ID';
COMMENT ON COLUMN favorites.conversation_id IS '会话ID（冗余字段，用于快速查询）';
COMMENT ON COLUMN favorites.tags IS '收藏标签（数组）';
COMMENT ON COLUMN favorites.note IS '收藏备注';
COMMENT ON COLUMN favorites.favorited_at IS '收藏时间';

-- 收藏表索引
CREATE INDEX IF NOT EXISTS idx_favorites_user_id ON favorites(user_id);
CREATE INDEX IF NOT EXISTS idx_favorites_message_id ON favorites(message_id);
CREATE INDEX IF NOT EXISTS idx_favorites_conversation_id ON favorites(conversation_id);
CREATE INDEX IF NOT EXISTS idx_favorites_favorited_at ON favorites(favorited_at DESC);
CREATE INDEX IF NOT EXISTS idx_favorites_tags ON favorites USING GIN(tags);  -- GIN索引支持数组查询

-- ============================================================================
-- 6. Hook引擎模块 (Hook Engine Module)
-- ============================================================================
-- 职责: Hook配置管理、动态配置存储、多租户支持

-- Hook配置表
-- COMMENT: Hook引擎核心表，存储Hook配置信息（动态API配置，最高优先级）
DROP TABLE IF EXISTS hook_configs CASCADE;
CREATE TABLE hook_configs (
    id BIGSERIAL PRIMARY KEY,
    hook_id TEXT UNIQUE,                  -- Hook ID（唯一标识，兼容旧版本）
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
    selector_config JSONB NOT NULL DEFAULT '{}',    -- 选择器配置（JSON格式，兼容 selector）
    transport_config JSONB NOT NULL,               -- 传输配置（JSON格式，兼容 transport）
    retry_policy JSONB DEFAULT '{}'::jsonb,        -- 重试策略（JSON格式，兼容旧版本）
    metadata JSONB,                                 -- 元数据（JSON格式）
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP,
    created_by TEXT,                                -- 创建者
    
    -- 唯一约束：同一租户下同一类型的Hook名称唯一
    UNIQUE(tenant_id, hook_type, name),
    -- 外键约束（可选）
    CONSTRAINT fk_hook_configs_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id) ON DELETE CASCADE
);

COMMENT ON TABLE hook_configs IS 'Hook配置表（动态API配置，最高优先级）';
COMMENT ON COLUMN hook_configs.id IS '配置ID（自增主键）';
COMMENT ON COLUMN hook_configs.tenant_id IS '租户ID（NULL表示全局配置，对所有租户生效）';
COMMENT ON COLUMN hook_configs.hook_type IS 'Hook类型（pre_send, post_send, delivery, recall, conversation_create, user_login等）';
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
COMMENT ON COLUMN hook_configs.selector_config IS '选择器配置（JSON格式，包含tenants, conversation_types, message_types等）';
COMMENT ON COLUMN hook_configs.transport_config IS '传输配置（JSON格式，包含type, endpoint等）';
COMMENT ON COLUMN hook_configs.metadata IS '元数据（JSON格式）';
COMMENT ON COLUMN hook_configs.created_at IS '创建时间';
COMMENT ON COLUMN hook_configs.updated_at IS '更新时间';
COMMENT ON COLUMN hook_configs.created_by IS '创建者';

-- Hook配置表索引
CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant_type ON hook_configs(tenant_id, hook_type, enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_priority ON hook_configs(priority DESC);
CREATE INDEX IF NOT EXISTS idx_hook_configs_hook_type ON hook_configs(hook_type);
CREATE INDEX IF NOT EXISTS idx_hook_configs_enabled ON hook_configs(enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_updated_at ON hook_configs(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant ON hook_configs(tenant_id);

-- Hook执行记录表（TimescaleDB Hypertable）
-- COMMENT: Hook执行记录表，记录Hook执行历史
DROP TABLE IF EXISTS hook_executions CASCADE;
CREATE TABLE hook_executions (
    execution_id TEXT PRIMARY KEY,
    hook_id TEXT NOT NULL,
    hook_name TEXT NOT NULL,
    hook_type TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    message_id TEXT,
    success BOOLEAN NOT NULL,
    latency_ms INTEGER,
    error_code TEXT,
    error_message TEXT,
    executed_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- SELECT create_hypertable('hook_executions', 'executed_at', 
--     chunk_time_interval => INTERVAL '1 day',
--     if_not_exists => TRUE);

CREATE INDEX IF NOT EXISTS idx_hook_executions_hook_id ON hook_executions(hook_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_tenant ON hook_executions(tenant_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_message_id ON hook_executions(message_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_executed_at ON hook_executions(executed_at);
CREATE INDEX IF NOT EXISTS idx_hook_executions_success ON hook_executions(success);

COMMENT ON TABLE hook_executions IS 'Hook执行记录表，记录Hook执行历史';

-- ============================================================================
-- 7. 连续聚合视图（TimescaleDB Continuous Aggregates）
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
    conversation_id,
    COUNT(*) AS message_count,
    COUNT(DISTINCT sender_id) AS unique_senders
FROM messages
GROUP BY hour, conversation_id;

COMMENT ON MATERIALIZED VIEW messages_hourly_stats IS '消息每小时统计视图（TimescaleDB连续聚合）';
COMMENT ON COLUMN messages_hourly_stats.hour IS '小时时间戳';
COMMENT ON COLUMN messages_hourly_stats.conversation_id IS '会话ID';
COMMENT ON COLUMN messages_hourly_stats.message_count IS '消息数量';
COMMENT ON COLUMN messages_hourly_stats.unique_senders IS '唯一发送者数量';

-- 设置连续聚合的刷新策略
-- COMMENT: 每小时刷新一次连续聚合视图，延迟3小时以确保数据完整性
-- 注意：不同版本的TimescaleDB可能使用不同的调用方式
-- 使用 DO 块来处理可能的函数不存在的情况
DO $$
BEGIN
    -- 尝试使用新版本的 CALL 语法
    BEGIN
        EXECUTE 'CALL add_continuous_aggregate_policy(''messages_hourly_stats'', 
            start_offset => INTERVAL ''3 hours'',
            end_offset => INTERVAL ''1 hour'', 
            schedule_interval => INTERVAL ''1 hour'')';
    EXCEPTION WHEN undefined_function OR syntax_error THEN
        -- 如果 CALL 不支持，尝试使用 SELECT
        BEGIN
            PERFORM add_continuous_aggregate_policy('messages_hourly_stats',
                start_offset => INTERVAL '3 hours',
                end_offset => INTERVAL '1 hour',
                schedule_interval => INTERVAL '1 hour');
        EXCEPTION WHEN undefined_function THEN
            -- 如果函数都不存在，跳过策略设置
            RAISE NOTICE 'add_continuous_aggregate_policy function not available, skipping continuous aggregate policy setup';
        END;
    END;
END $$;

-- ============================================================================
-- 8. 触发器（自动更新时间戳）
-- ============================================================================

-- 会话表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_conversations_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_conversations_updated_at
    BEFORE UPDATE ON conversations
    FOR EACH ROW
    EXECUTE FUNCTION update_conversations_updated_at();

-- 会话参与者表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_conversation_participants_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_conversation_participants_updated_at
    BEFORE UPDATE ON conversation_participants
    FOR EACH ROW
    EXECUTE FUNCTION update_conversation_participants_updated_at();

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

-- message_state 表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_message_state_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_message_state_updated_at
    BEFORE UPDATE ON message_state
    FOR EACH ROW
    EXECUTE FUNCTION update_message_state_updated_at();

-- message_operation_history 表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_message_operation_history_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_message_operation_history_updated_at
    BEFORE UPDATE ON message_operation_history
    FOR EACH ROW
    EXECUTE FUNCTION update_message_operation_history_updated_at();

-- threads 表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_threads_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_threads_updated_at
    BEFORE UPDATE ON threads
    FOR EACH ROW
    EXECUTE FUNCTION update_threads_updated_at();

-- favorites 表更新时间戳触发器
CREATE OR REPLACE FUNCTION update_favorites_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER trigger_favorites_updated_at
    BEFORE UPDATE ON favorites
    FOR EACH ROW
    EXECUTE FUNCTION update_favorites_updated_at();

-- ============================================================================
-- 初始化完成
-- ============================================================================
