-- 统一网关数据库表结构
-- 创建时间: 2025-01-XX

-- 租户表
CREATE TABLE IF NOT EXISTS tenants (
    tenant_id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    status VARCHAR(32) NOT NULL DEFAULT 'active',
    config JSONB DEFAULT '{}'::jsonb,
    quota JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tenants_status ON tenants(status);
CREATE INDEX IF NOT EXISTS idx_tenants_created_at ON tenants(created_at);

-- Hook配置表
CREATE TABLE IF NOT EXISTS hook_configs (
    hook_id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    hook_type VARCHAR(32) NOT NULL,
    tenant_id VARCHAR(64) NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    enabled BOOLEAN DEFAULT TRUE,
    transport JSONB NOT NULL,
    selector JSONB DEFAULT '{}'::jsonb,
    retry_policy JSONB DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW(),
    CONSTRAINT fk_tenant FOREIGN KEY (tenant_id) REFERENCES tenants(tenant_id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant ON hook_configs(tenant_id);
CREATE INDEX IF NOT EXISTS idx_hook_configs_type ON hook_configs(hook_type);
CREATE INDEX IF NOT EXISTS idx_hook_configs_enabled ON hook_configs(enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant_type ON hook_configs(tenant_id, hook_type);
CREATE INDEX IF NOT EXISTS idx_hook_configs_priority ON hook_configs(priority DESC);

-- Hook执行记录表（TimescaleDB Hypertable）
CREATE TABLE IF NOT EXISTS hook_executions (
    execution_id VARCHAR(64) PRIMARY KEY,
    hook_id VARCHAR(64) NOT NULL,
    hook_name VARCHAR(255) NOT NULL,
    hook_type VARCHAR(32) NOT NULL,
    tenant_id VARCHAR(64) NOT NULL,
    message_id VARCHAR(64),
    success BOOLEAN NOT NULL,
    latency_ms INTEGER,
    error_code VARCHAR(32),
    error_message TEXT,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 创建TimescaleDB Hypertable（如果TimescaleDB已安装）
-- SELECT create_hypertable('hook_executions', 'executed_at', 
--     chunk_time_interval => INTERVAL '1 day');

CREATE INDEX IF NOT EXISTS idx_hook_executions_hook_id ON hook_executions(hook_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_tenant ON hook_executions(tenant_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_executed_at ON hook_executions(executed_at);
CREATE INDEX IF NOT EXISTS idx_hook_executions_success ON hook_executions(success);

-- 添加注释
COMMENT ON TABLE tenants IS '租户表，存储租户基本信息、配置和配额';
COMMENT ON TABLE hook_configs IS 'Hook配置表，存储Hook配置信息';
COMMENT ON TABLE hook_executions IS 'Hook执行记录表，记录Hook执行历史';

COMMENT ON COLUMN tenants.status IS '租户状态：active, suspended, deleted';
COMMENT ON COLUMN tenants.config IS '租户配置（JSON格式）';
COMMENT ON COLUMN tenants.quota IS '租户配额（JSON格式）';
COMMENT ON COLUMN hook_configs.hook_type IS 'Hook类型：pre_send, post_send, delivery, recall';
COMMENT ON COLUMN hook_configs.transport IS '传输配置（JSON格式）：grpc, webhook, local';
COMMENT ON COLUMN hook_configs.selector IS '选择器配置（JSON格式）：租户、会话类型、消息类型匹配规则';
COMMENT ON COLUMN hook_configs.retry_policy IS '重试策略（JSON格式）';

