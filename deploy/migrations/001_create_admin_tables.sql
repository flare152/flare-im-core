-- 创建租户表
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

-- 创建Hook配置表
CREATE TABLE IF NOT EXISTS hook_configs (
    hook_id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    hook_type VARCHAR(32) NOT NULL,
    tenant_id VARCHAR(64) NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    enabled BOOLEAN DEFAULT TRUE,
    transport JSONB NOT NULL,
    selector JSONB,
    retry_policy JSONB,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant ON hook_configs(tenant_id);
CREATE INDEX IF NOT EXISTS idx_hook_configs_type ON hook_configs(hook_type);
CREATE INDEX IF NOT EXISTS idx_hook_configs_enabled ON hook_configs(enabled);
CREATE INDEX IF NOT EXISTS idx_hook_configs_tenant_type ON hook_configs(tenant_id, hook_type);

-- 创建Hook执行记录表（TimescaleDB Hypertable）
CREATE TABLE IF NOT EXISTS hook_executions (
    execution_id VARCHAR(64) PRIMARY KEY,
    hook_id VARCHAR(64) NOT NULL,
    message_id VARCHAR(64),
    success BOOLEAN NOT NULL,
    latency_ms INTEGER,
    error_code VARCHAR(32),
    error_message TEXT,
    executed_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 转换为 Hypertable（如果使用TimescaleDB）
-- SELECT create_hypertable('hook_executions', 'executed_at', 
--     chunk_time_interval => INTERVAL '1 day',
--     if_not_exists => TRUE);

CREATE INDEX IF NOT EXISTS idx_hook_executions_hook_id ON hook_executions(hook_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_message_id ON hook_executions(message_id);
CREATE INDEX IF NOT EXISTS idx_hook_executions_executed_at ON hook_executions(executed_at);
CREATE INDEX IF NOT EXISTS idx_hook_executions_success ON hook_executions(success);

-- 创建告警规则表
CREATE TABLE IF NOT EXISTS alert_rules (
    rule_id VARCHAR(64) PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    metric_name VARCHAR(255) NOT NULL,
    condition VARCHAR(32) NOT NULL,
    threshold VARCHAR(64) NOT NULL,
    duration_seconds INTEGER NOT NULL DEFAULT 300,
    notification_channels TEXT[],
    enabled BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_alert_rules_enabled ON alert_rules(enabled);
CREATE INDEX IF NOT EXISTS idx_alert_rules_metric_name ON alert_rules(metric_name);

-- 创建告警历史表（TimescaleDB Hypertable）
CREATE TABLE IF NOT EXISTS alert_history (
    alert_id VARCHAR(64) PRIMARY KEY,
    rule_id VARCHAR(64) NOT NULL,
    metric_name VARCHAR(255) NOT NULL,
    current_value DOUBLE PRECISION NOT NULL,
    threshold VARCHAR(64) NOT NULL,
    severity VARCHAR(32) NOT NULL,
    triggered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

-- SELECT create_hypertable('alert_history', 'triggered_at', 
--     chunk_time_interval => INTERVAL '1 day',
--     if_not_exists => TRUE);

CREATE INDEX IF NOT EXISTS idx_alert_history_rule_id ON alert_history(rule_id);
CREATE INDEX IF NOT EXISTS idx_alert_history_triggered_at ON alert_history(triggered_at);
CREATE INDEX IF NOT EXISTS idx_alert_history_resolved_at ON alert_history(resolved_at);

-- 添加注释
COMMENT ON TABLE tenants IS '租户信息表';
COMMENT ON TABLE hook_configs IS 'Hook配置表';
COMMENT ON TABLE hook_executions IS 'Hook执行记录表';
COMMENT ON TABLE alert_rules IS '告警规则表';
COMMENT ON TABLE alert_history IS '告警历史表';

