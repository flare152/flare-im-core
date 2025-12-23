//! Router 中枢组件（调度层）
//!
//! 负责分片路由、负载均衡、流控、跨机房选择与 Trace 注入。

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use redis::{AsyncCommands, aio::ConnectionManager};
use uuid::Uuid;

use crate::domain::repository::RouteRepository;
use crate::service::metrics::RouterMetrics;
use flare_server_core::discovery::ServiceInstance;

/// 监控客户端trait，用于查询系统反压信号
#[async_trait::async_trait]
pub trait MonitoringClient: Send + Sync {
    /// 获取Kafka Lag值
    async fn get_kafka_lag(&self) -> anyhow::Result<u64>;

    /// 获取Storage写入延迟（P99）
    async fn get_storage_latency(&self) -> anyhow::Result<f64>;
}

/// 轻量的路由上下文（从 RequestContext/Metadata 提取）
#[derive(Debug, Clone, Default)]
pub struct RouteContext {
    pub svid: String,
    pub conversation_id: Option<String>,
    pub user_id: Option<String>,
    pub tenant_id: Option<String>,
    pub client_geo: Option<String>,
    pub login_gateway: Option<String>,
}

/// 分片管理
#[derive(Clone)]
pub struct ShardManager {
    shard_count: usize,
}

impl ShardManager {
    pub fn new(shard_count: usize) -> Self {
        Self { shard_count }
    }

    pub fn pick_shard(&self, conversation_id: Option<&str>, user_id: Option<&str>) -> usize {
        let key = conversation_id.or(user_id).unwrap_or("default");
        // 简易 Murmur3 替代：使用 Rust 默认哈希（可替换为真正 murmur3）
        let mut hash: u64 = 1469598103934665603; // FNV offset basis
        for b in key.as_bytes() {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
        (hash % self.shard_count.max(1) as u64) as usize
    }
}

/// 负载均衡器（支持轮询、最小连接、延迟感知）
///
/// 参考飞书Lark-Dispatcher设计：
/// - 轮询（Round Robin）：默认策略
/// - 最小连接（Least Connections）：动态负载感知
/// - 延迟感知（Latency-Aware）：P99延迟择优
/// - 权重路由（Weighted）：金丝雀发布/灰度流量
#[derive(Clone)]
pub struct ServiceLoadBalancer {
    /// 负载均衡策略
    strategy: LoadBalancingStrategy,
    /// 轮询计数器（用于RoundRobin）
    robin_counter: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// 实例指标缓存（用于LeastConnections和LatencyAware策略）
    metrics_cache: std::sync::Arc<std::sync::Mutex<HashMap<String, HashMap<String, u64>>>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadBalancingStrategy {
    /// 轮询（默认）
    RoundRobin,
    /// 最小连接（需外部提供连接数指标）
    LeastConnections,
    /// 延迟感知（需外部提供P99延迟指标）
    LatencyAware,
}

impl ServiceLoadBalancer {
    pub fn new() -> Self {
        Self {
            strategy: LoadBalancingStrategy::RoundRobin,
            robin_counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            metrics_cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    pub fn with_strategy(strategy: LoadBalancingStrategy) -> Self {
        Self {
            strategy,
            robin_counter: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            metrics_cache: std::sync::Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// 获取实例指标（从缓存中获取，如果不存在则返回默认值）
    fn get_instance_metric(&self, instance_id: &str, metric_name: &str) -> u64 {
        let cache = self.metrics_cache.lock().unwrap();
        if let Some(instance_metrics) = cache.get(instance_id) {
            *instance_metrics.get(metric_name).unwrap_or(&0u64)
        } else {
            0
        }
    }

    /// 更新实例指标缓存
    pub fn update_instance_metrics(&self, instance_id: String, metrics: HashMap<String, u64>) {
        let mut cache = self.metrics_cache.lock().unwrap();
        cache.insert(instance_id, metrics);
    }

    /// 选择服务实例（业务感知的负载均衡）
    ///
    /// # 关键设计
    ///
    /// **与 etcd/Consul/Service Mesh 的职责边界**：
    /// - etcd/Consul: 提供候选实例列表（带 shard_id、az 元数据）
    /// - Service Mesh: 提供通用 L7 负载均衡（Round Robin、熔断、重试）
    /// - **Router 负载均衡**: 业务感知路由（分片亲和、流控、跨机房智能选择）
    ///
    /// # 为什么 Router 需要自己的负载均衡？
    ///
    /// 1. **分片亲和性**: 同一会话/用户必须路由到同一个 shard 实例，etcd 无法保证
    /// 2. **跨机房智能选择**: 需结合用户地理、机房负载、存储健康度，Service Mesh 无业务感知
    /// 3. **流控与降级**: 根据会话级 QPS、群聊 fanout 动态限流，etcd 无此能力
    /// 4. **A/B 测试**: 根据租户 ID、用户标签灰度路由，Service Mesh 仅支持流量百分比
    ///
    /// # 参数
    ///
    /// - `candidates`: 从 etcd/Consul 获取的候选实例列表（已包含 shard_id、az 元数据）
    /// - `shard`: 根据 conversation_id/user_id 计算的目标分片 ID
    /// - `target_az`: 跨机房智能选择的目标机房（可选）
    ///
    /// # 返回
    ///
    /// - 符合分片亲和性 + 机房亲和性的实例
    /// - 在同 shard + 同 az 的实例内根据策略（RoundRobin/LeastConnections/LatencyAware）选择
    pub fn pick_service_from_instances(
        &self,
        candidates: &[ServiceInstance],
        shard: usize,
        target_az: Option<&str>,
    ) -> Option<ServiceInstance> {
        if candidates.is_empty() {
            return None;
        }

        // 1. 按 shard_id 过滤实例（分片亲和性）
        let shard_filtered: Vec<&ServiceInstance> = candidates
            .iter()
            .filter(|inst| {
                // 从 metadata.custom 中提取 shard_id（etcd/Consul 注册时带上）
                inst.metadata
                    .custom
                    .get("shard_id")
                    .and_then(|s| s.parse::<usize>().ok())
                    .map(|inst_shard| inst_shard == shard)
                    .unwrap_or(false)
            })
            .collect();

        // 如果没有匹配的 shard，降级到所有候选（警告日志）
        let shard_candidates: Vec<&ServiceInstance> = if shard_filtered.is_empty() {
            tracing::warn!(
                shard = shard,
                total_candidates = candidates.len(),
                "No instances found for shard, falling back to all candidates"
            );
            candidates.iter().collect()
        } else {
            shard_filtered
        };

        // 2. 按 az 过滤实例（跨机房亲和性）
        let final_candidates: Vec<&ServiceInstance> = if let Some(az) = target_az {
            let az_filtered: Vec<&ServiceInstance> = shard_candidates
                .iter()
                .filter(|inst| {
                    // 优先使用 metadata.zone（标准字段），其次使用 metadata.custom["az"]
                    inst.metadata
                        .zone
                        .as_ref()
                        .map(|z| z == az)
                        .or_else(|| inst.metadata.custom.get("az").map(|inst_az| inst_az == az))
                        .unwrap_or(false)
                })
                .copied()
                .collect();

            // 如果没有匹配的 az，降级到 shard 候选
            if az_filtered.is_empty() {
                tracing::debug!(
                    target_az = ?target_az,
                    shard = shard,
                    "No instances found for target AZ, falling back to shard candidates"
                );
                shard_candidates
            } else {
                az_filtered
            }
        } else {
            shard_candidates
        };

        // 3. 在最终候选中根据策略选择实例
        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                let index = self
                    .robin_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                final_candidates
                    .get(index % final_candidates.len())
                    .map(|inst| (*inst).clone())
            }
            LoadBalancingStrategy::LeastConnections => {
                // 实现基于连接数的负载均衡
                let selected = final_candidates
                    .iter()
                    .min_by_key(|inst| {
                        self.get_instance_metric(&inst.instance_id, "active_connections")
                    })
                    .map(|inst| (*inst).clone());

                if let Some(ref selected_instance) = selected {
                    let connection_count = self
                        .get_instance_metric(&selected_instance.instance_id, "active_connections");
                    tracing::info!(
                        selected_instance = %selected_instance.instance_id,
                        connection_count = connection_count,
                        "LeastConnections strategy selected"
                    );
                } else {
                    tracing::warn!("LeastConnections strategy: no valid candidates");
                }

                selected
            }
            LoadBalancingStrategy::LatencyAware => {
                // 实现基于延迟的智能负载均衡
                let selected = final_candidates
                    .iter()
                    .min_by_key(|inst| self.get_instance_metric(&inst.instance_id, "p99_latency"))
                    .map(|inst| (*inst).clone());

                if let Some(ref selected_instance) = selected {
                    let latency =
                        self.get_instance_metric(&selected_instance.instance_id, "p99_latency");
                    tracing::info!(
                        selected_instance = %selected_instance.instance_id,
                        latency_ms = latency,
                        "LatencyAware strategy selected"
                    );
                } else {
                    tracing::warn!("LatencyAware strategy: no valid candidates");
                }

                selected
            }
        }
    }

    /// 兼容旧接口：从字符串列表选择（仅用于向后兼容）
    ///
    /// **注意**: 此接口无法感知分片和机房，仅用于简单场景或测试
    pub fn pick_service(
        &self,
        _svid: &str,
        _shard: usize,
        candidates: &[String],
    ) -> Option<String> {
        if candidates.is_empty() {
            return None;
        }

        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                let index = self
                    .robin_counter
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Some(candidates[index % candidates.len()].clone())
            }
            LoadBalancingStrategy::LeastConnections => {
                // 生产实现：查询指标缓存获取各服务实例的当前连接数
                let selected = candidates.iter().min_by_key(|endpoint| {
                    // 从endpoint中提取instance_id（假设格式为http://host:port）
                    let instance_id = endpoint.replace("http://", "").replace("https://", "");
                    self.get_instance_metric(&instance_id, "active_connections")
                });

                if let Some(selected_endpoint) = selected {
                    tracing::trace!("LeastConnections strategy selected: {}", selected_endpoint);
                    Some(selected_endpoint.clone())
                } else {
                    tracing::trace!(
                        "LeastConnections strategy (fallback: returning first candidate)"
                    );
                    candidates.first().cloned()
                }
            }
            LoadBalancingStrategy::LatencyAware => {
                // 生产实现：查询指标缓存获取各服务实例的P99延迟
                let selected = candidates.iter().min_by_key(|endpoint| {
                    // 从endpoint中提取instance_id（假设格式为http://host:port）
                    let instance_id = endpoint.replace("http://", "").replace("https://", "");
                    self.get_instance_metric(&instance_id, "p99_latency")
                });

                if let Some(selected_endpoint) = selected {
                    tracing::trace!("LatencyAware strategy selected: {}", selected_endpoint);
                    Some(selected_endpoint.clone())
                } else {
                    tracing::trace!("LatencyAware strategy (fallback: returning first candidate)");
                    candidates.first().cloned()
                }
            }
        }
    }
}

/// 流控器（会话/群聊限速 + 反压）
///
/// 参考微信MsgService流控设计：
/// - 会话级QPS限制（防止单会话攻击）
/// - 群聊fanout限速（大群消息控制）
/// - 系统级反压（Kafka/Storage过载时降级）
/// - 热点会话自动降级
#[derive(Clone)]
pub struct FlowController {
    /// 会话级QPS限制（默认50 QPS）
    session_qps_limit: u32,
    /// 群聊fanout限制（默认2000用户/秒）
    group_fanout_limit: u32,
    /// 是否启用反压
    backpressure_enabled: bool,
    /// 热点会话阈值（QPS超过此值自动降级）
    hot_session_threshold: u32,
    /// Redis客户端（用于滑动窗口计数器）
    redis_client: Option<Arc<redis::Client>>,
    /// 监控系统客户端（用于查询反压信号）
    monitoring_client: Option<Arc<dyn MonitoringClient + Send + Sync>>,
    /// 热点会话缓存（用于热点检测和降级）
    hot_sessions:
        std::sync::Arc<parking_lot::RwLock<std::collections::HashMap<String, HotSessionInfo>>>,
}

/// 热点会话信息
#[derive(Debug, Clone)]
struct HotSessionInfo {
    /// 最后一次检测到的时间（使用系统时间戳）
    last_detected: std::time::SystemTime,
    /// 当前QPS
    current_qps: u32,
    /// 是否已被降级
    degraded: bool,
}

impl FlowController {
    pub fn new() -> Self {
        Self {
            session_qps_limit: 50,
            group_fanout_limit: 2000,
            backpressure_enabled: true,
            hot_session_threshold: 100,
            redis_client: None,
            monitoring_client: None,
            hot_sessions: std::sync::Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    pub fn with_limits(session_qps: u32, group_fanout: u32, hot_threshold: u32) -> Self {
        Self {
            session_qps_limit: session_qps,
            group_fanout_limit: group_fanout,
            backpressure_enabled: true,
            hot_session_threshold: hot_threshold,
            redis_client: None,
            monitoring_client: None,
            hot_sessions: std::sync::Arc::new(parking_lot::RwLock::new(
                std::collections::HashMap::new(),
            )),
        }
    }

    /// 设置Redis客户端
    pub fn with_redis_client(mut self, redis_client: Arc<redis::Client>) -> Self {
        self.redis_client = Some(redis_client);
        self
    }

    /// 设置监控系统客户端
    pub fn with_monitoring_client(
        mut self,
        monitoring_client: Arc<dyn MonitoringClient + Send + Sync>,
    ) -> Self {
        self.monitoring_client = Some(monitoring_client);
        self
    }

    /// 获取Redis连接
    async fn get_redis_connection(&self) -> Result<ConnectionManager> {
        if let Some(client) = &self.redis_client {
            Ok(ConnectionManager::new(client.as_ref().clone()).await?)
        } else {
            Err(anyhow::anyhow!("Redis client not configured"))
        }
    }

    /// 查询会话QPS（使用Redis滑动窗口算法）
    ///
    /// 实现滑动窗口计数器算法：
    /// 1. 使用Redis的INCR命令增加计数器
    /// 2. 使用EXPIRE设置过期时间，实现窗口滑动
    /// 3. 返回当前计数器值作为QPS估算
    async fn get_session_qps(&self, conversation_id: &str) -> Result<u32> {
        if self.redis_client.is_none() {
            return Ok(0);
        }

        let mut conn = self.get_redis_connection().await?;
        let key = format!("rate_limit:session_qps:{}", conversation_id);

        // 增加计数器并设置过期时间（1秒窗口）
        let count: u32 = conn.incr(&key, 1).await?;

        // 如果是第一次设置，则设置过期时间
        if count == 1 {
            let _: () = conn.expire(&key, 1).await?;
        }

        Ok(count)
    }

    /// 查询Kafka Lag（反压信号之一）
    async fn get_kafka_lag(&self) -> Result<u64> {
        if let Some(client) = &self.monitoring_client {
            client.get_kafka_lag().await
        } else {
            Ok(0)
        }
    }

    /// 查询Storage写入延迟（反压信号之一）
    async fn get_storage_latency(&self) -> Result<f64> {
        if let Some(client) = &self.monitoring_client {
            client.get_storage_latency().await
        } else {
            Ok(0.0)
        }
    }

    /// 检查是否为热点会话
    fn is_hot_session(&self, conversation_id: &str, current_qps: u32) -> bool {
        current_qps > self.hot_session_threshold
    }

    /// 更新热点会话信息
    fn update_hot_session(&self, conversation_id: &str, current_qps: u32) {
        let mut hot_sessions = self.hot_sessions.write();
        let info = HotSessionInfo {
            last_detected: std::time::SystemTime::now(),
            current_qps,
            degraded: current_qps > self.hot_session_threshold,
        };
        hot_sessions.insert(conversation_id.to_string(), info);
    }

    /// 检查会话是否已被降级
    fn is_session_degraded(&self, conversation_id: &str) -> bool {
        let hot_sessions = self.hot_sessions.read();
        if let Some(info) = hot_sessions.get(conversation_id) {
            info.degraded
        } else {
            false
        }
    }

    /// 流控检查（生产实现应结合 Redis + 滑动窗口）
    ///
    /// 当前为占位实现，生产环境需集成：
    /// 1. Redis 滑动窗口计数器（会话级QPS）
    /// 2. Kafka Lag 监控（反压信号）
    /// 3. Storage 写入延迟（健康度检测）
    pub async fn check(&self, ctx: &RouteContext) -> Result<()> {
        // 生产实现：
        // 1. 检查会话QPS（Redis INCR + EXPIRE）
        // 使用滑动窗口算法检查会话级QPS是否超过限制
        let session_qps = if let Some(conversation_id) = &ctx.conversation_id {
            let qps = self.get_session_qps(conversation_id).await?;
            // 检查是否为热点会话
            if self.is_hot_session(conversation_id, qps) {
                self.update_hot_session(conversation_id, qps);
                // 如果会话已被降级，则延迟推送
                if self.is_session_degraded(conversation_id) {
                    // 延迟推送逻辑（可以根据需要调整延迟时间）
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            }
            qps
        } else {
            0
        };

        if let Some(conversation_id) = &ctx.conversation_id {
            if session_qps > self.session_qps_limit {
                return Err(anyhow::anyhow!(
                    "Session QPS limit exceeded: {} > {}",
                    session_qps,
                    self.session_qps_limit
                ));
            }
        }

        // 2. 检查群聊fanout（大群消息批次限制）
        // 对于群聊消息，检查接收者数量是否超过限制
        // 注意：这里的实现需要从ctx中获取消息类型和接收者信息
        // 在实际实现中，可能需要额外的参数来传递这些信息

        // 3. 检查系统反压信号（Kafka Lag > 10000 或 Storage P99 > 500ms）
        if self.backpressure_enabled {
            // 检查Kafka Lag
            let kafka_lag = self.get_kafka_lag().await?;
            if kafka_lag > 10000 {
                return Err(anyhow::anyhow!("Kafka lag too high: {}", kafka_lag));
            }

            // 检查Storage写入延迟
            let storage_latency = self.get_storage_latency().await?;
            if storage_latency > 500.0 {
                return Err(anyhow::anyhow!(
                    "Storage latency too high: {}ms",
                    storage_latency
                ));
            }
        }

        // 4. 热点会话降级（QPS > hot_session_threshold 时延迟推送）
        // 已在上面的QPS检查中实现

        tracing::trace!(
            conversation_id = ?ctx.conversation_id,
            svid = %ctx.svid,
            session_qps,
            "Flow control check passed"
        );
        Ok(())
    }
}

/// 跨机房智能选择器（Multi-AZ Routing）
///
/// 参考微信/Telegram的跨DC路由策略：
/// - 优先用户地理位置（GeoIP）
/// - 其次登录网关所在机房（就近原则）
/// - 机房负载/延迟/存储健康度综合评分
/// - 弱网环境优先近端机房
#[derive(Clone)]
pub struct AzSelector {
    /// 默认机房（兜底）
    default_az: String,
    /// 机房优先级配置（geo -> az 映射）
    /// 例如：{"CN-East": "shanghai", "CN-North": "beijing"}
    geo_az_map: std::collections::HashMap<String, String>,
    /// 配置中心客户端（用于查询租户偏好机房）
    config_client: Option<Arc<dyn ConfigClient + Send + Sync>>,
}

/// 配置中心客户端trait，用于查询租户偏好机房
#[async_trait::async_trait]
pub trait ConfigClient: Send + Sync {
    /// 获取租户偏好机房
    async fn get_tenant_preferred_az(&self, tenant_id: &str) -> anyhow::Result<Option<String>>;
}

impl AzSelector {
    pub fn new() -> Self {
        Self {
            default_az: "default".to_string(),
            geo_az_map: std::collections::HashMap::new(),
            config_client: None,
        }
    }

    pub fn with_config(
        default_az: String,
        geo_az_map: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            default_az,
            geo_az_map,
            config_client: None,
        }
    }

    /// 设置配置中心客户端
    pub fn with_config_client(
        mut self,
        config_client: Arc<dyn ConfigClient + Send + Sync>,
    ) -> Self {
        self.config_client = Some(config_client);
        self
    }

    /// 选择最优机房（生产实现应结合实时健康度）
    ///
    /// 当前为占位实现，生产环境需集成：
    /// 1. GeoIP 查询（client_geo -> 最近机房）
    /// 2. 登录网关机房标签（gateway_id 带机房后缀，如 gateway-sh-1）
    /// 3. 机房健康度监控（存储延迟、CPU、网络带宽）
    /// 4. 租户级机房亲和性（大客户专属机房）
    pub fn pick(
        &self,
        client_geo: Option<&str>,
        login_gateway: Option<&str>,
        tenant_id: Option<&str>,
    ) -> Option<String> {
        // 1. 优先根据地理位置选择
        if let Some(geo) = client_geo {
            if let Some(az) = self.geo_az_map.get(geo) {
                tracing::debug!(geo = %geo, az = %az, "Selected AZ by client geo");
                return Some(az.clone());
            }
        }

        // 2. 根据登录网关提取机房（例如 gateway-sh-1 -> shanghai）
        if let Some(gateway) = login_gateway {
            if let Some(az) = self.extract_az_from_gateway(gateway) {
                tracing::debug!(gateway = %gateway, az = %az, "Selected AZ by login gateway");
                return Some(az);
            }
        }

        // 3. 租户级机房亲和性（查询配置中心）
        if let (Some(config_client), Some(tenant_id)) = (&self.config_client, tenant_id) {
            // 在实际实现中，这里应该是异步调用
            // 但由于pick方法是同步的，我们需要一种方式来处理异步调用
            // 这里我们简化处理，假设有一个同步的方法或者缓存机制
            tracing::debug!(tenant_id = %tenant_id, "Checking tenant preferred AZ");
            // 注意：实际实现应该查询配置中心获取租户偏好机房
        }

        // 4. 兜底：使用默认机房
        tracing::debug!(az = %self.default_az, "Using default AZ");
        Some(self.default_az.clone())
    }

    /// 从网关ID提取机房标识（例如 gateway-sh-1 -> shanghai）
    fn extract_az_from_gateway(&self, gateway: &str) -> Option<String> {
        // 简易实现：提取 gateway-{az}-{num} 中的 az 部分
        let parts: Vec<&str> = gateway.split('-').collect();
        if parts.len() >= 3 {
            Some(parts[1].to_string())
        } else {
            None
        }
    }
}

/// Trace 注入器
#[derive(Clone)]
pub struct TraceInjector;

impl TraceInjector {
    pub fn new() -> Self {
        Self
    }
    pub fn inject(&self, _ctx: &RouteContext) -> String {
        Uuid::new_v4().to_string()
    }
}

/// Router 聚合体（IM系统的调度中枢）
///
/// # 核心定位
///
/// Router 不是简单的 SVID → 服务映射，而是 IM 全链路的：
/// - **路由中枢**：SVID路由 + 分片调度
/// - **流控引擎**：会话QPS + 群聊fanout + 系统反压
/// - **跨机房选择**：地理/负载/健康度智能路由
/// - **负载均衡**：轮询/最小连接/延迟感知
/// - **Trace注入**：全链路跟踪与监控
///
/// # 对标产品
///
/// - 微信：MsgService Router
/// - 飞书：Lark-Dispatcher
/// - Discord：Edge Gateway + Dispatch Layer
/// - Telegram：DC Route + Message Distributor
///
/// # 与 Gateway 的边界
///
/// | 模块 | 职责 | 不做什么 |
/// |---------|------|------------|
/// | Gateway | 长连接、基本鉴权、协议解包 | 不做业务路由、不做分片、不做流控 |
/// | Router  | 服务发现、分片路由、消息调度、编排与流控 | 不维护连接、不直接与客户端交互 |
///
/// Router 是"业务路由"，Gateway 是"传输层"。
#[derive(Clone)]
pub struct Router {
    shard_manager: ShardManager,
    service_lb: ServiceLoadBalancer,
    flow_controller: FlowController,
    az_selector: AzSelector,
    trace_injector: TraceInjector,
    metrics: Option<Arc<RouterMetrics>>,
    redis_client: Option<Arc<redis::Client>>,
    monitoring_client: Option<Arc<dyn MonitoringClient + Send + Sync>>,
    config_client: Option<Arc<dyn ConfigClient + Send + Sync>>,
}

impl Router {
    pub fn new(shard_count: usize) -> Arc<Self> {
        Arc::new(Self {
            shard_manager: ShardManager::new(shard_count),
            service_lb: ServiceLoadBalancer::new(),
            flow_controller: FlowController::new(),
            az_selector: AzSelector::new(),
            trace_injector: TraceInjector::new(),
            metrics: None,
            redis_client: None,
            monitoring_client: None,
            config_client: None,
        })
    }

    pub fn with_metrics(shard_count: usize, metrics: Arc<RouterMetrics>) -> Arc<Self> {
        Arc::new(Self {
            shard_manager: ShardManager::new(shard_count),
            service_lb: ServiceLoadBalancer::new(),
            flow_controller: FlowController::new(),
            az_selector: AzSelector::new(),
            trace_injector: TraceInjector::new(),
            metrics: Some(metrics),
            redis_client: None,
            monitoring_client: None,
            config_client: None,
        })
    }

    /// 设置Redis客户端
    pub fn with_redis_client(self: Arc<Self>, redis_client: Arc<redis::Client>) -> Arc<Self> {
        // 创建新的Router实例，复制所有字段
        let router = self.as_ref();
        Arc::new(Self {
            shard_manager: router.shard_manager.clone(),
            service_lb: router.service_lb.clone(),
            flow_controller: router
                .flow_controller
                .clone()
                .with_redis_client(redis_client.clone()),
            az_selector: router.az_selector.clone(),
            trace_injector: router.trace_injector.clone(),
            metrics: router.metrics.clone(),
            redis_client: Some(redis_client),
            monitoring_client: router.monitoring_client.clone(),
            config_client: router.config_client.clone(),
        })
    }

    /// 设置监控客户端
    pub fn with_monitoring_client(
        self: Arc<Self>,
        monitoring_client: Arc<dyn MonitoringClient + Send + Sync>,
    ) -> Arc<Self> {
        // 创建新的Router实例，复制所有字段
        let router = self.as_ref();
        Arc::new(Self {
            shard_manager: router.shard_manager.clone(),
            service_lb: router.service_lb.clone(),
            flow_controller: router
                .flow_controller
                .clone()
                .with_monitoring_client(monitoring_client.clone()),
            az_selector: router.az_selector.clone(),
            trace_injector: router.trace_injector.clone(),
            metrics: router.metrics.clone(),
            redis_client: router.redis_client.clone(),
            monitoring_client: Some(monitoring_client),
            config_client: router.config_client.clone(),
        })
    }

    /// 设置配置中心客户端
    pub fn with_config_client(
        self: Arc<Self>,
        config_client: Arc<dyn ConfigClient + Send + Sync>,
    ) -> Arc<Self> {
        // 创建新的Router实例，复制所有字段
        let router = self.as_ref();
        Arc::new(Self {
            shard_manager: router.shard_manager.clone(),
            service_lb: router.service_lb.clone(),
            flow_controller: router.flow_controller.clone(),
            az_selector: router
                .az_selector
                .clone()
                .with_config_client(config_client.clone()),
            trace_injector: router.trace_injector.clone(),
            metrics: router.metrics.clone(),
            redis_client: router.redis_client.clone(),
            monitoring_client: router.monitoring_client.clone(),
            config_client: Some(config_client),
        })
    }

    /// 解析端点（核心路由逻辑）
    ///
    /// # 流程
    ///
    /// 1. **Trace 注入**：生成 trace_id 并传播到下游
    /// 2. **流控检查**：会话QPS + 群聊fanout + 系统反压
    /// 3. **分片选择**：`shard_id = hash(conversation_id|user_id) % N`
    /// 4. **跨机房选择**：基于地理/负载/健康度（可选）
    /// 5. **服务发现**：从 etcd/Consul 获取候选实例列表（带 shard_id、az 元数据）
    /// 6. **业务负载均衡**：在同 shard + 同 az 的实例内根据策略选择
    /// 7. **返回端点**：gRPC 地址
    ///
    /// # 参数
    ///
    /// - `ctx`: 路由上下文（SVID、conversation_id、user_id、tenant_id、geo等）
    /// - `repository`: 路由表仓储（SVID → endpoint 映射，或 SVID → 服务发现配置）
    ///
    /// # 返回
    ///
    /// - `Ok(String)`: 目标服务端点（例如 `http://flare-conversation:8080`）
    /// - `Err`: 流控拒绝/路由表不存在/服务不可用
    ///
    /// # 与 etcd/Consul/Service Mesh 的集成
    ///
    /// **模式 1: 基础设施 + 业务路由**（当前推荐）
    /// ```text
    /// Router 中枢
    ///   ↓ (查询服务发现)
    /// etcd/Consul (提供候选实例列表 + shard_id/az 元数据)
    ///   ↓ (Router 根据业务规则选择实例)
    /// 服务实例 (flare-conversation-shard-0、shard-1...)
    /// ```
    ///
    /// **模式 2: Service Mesh + 业务路由**（大厂混合模式）
    /// ```text
    /// Router 中枢 (计算 shard_id)
    ///   ↓ (gRPC 带 shard_id header)
    /// Service Mesh (Istio/Linkerd，根据 header 路由到 subset)
    ///   ↓ (在同 shard 内 L7 负载均衡)
    /// 服务实例 (按 shard_id 分组)
    /// ```
    pub async fn resolve_endpoint(
        &self,
        ctx: &RouteContext,
        repository: Arc<dyn RouteRepository + Send + Sync>,
    ) -> Result<String> {
        let start = Utc::now();
        // 1. Trace 注入
        let trace_id = self.trace_injector.inject(ctx);
        tracing::debug!(trace_id = %trace_id, svid = %ctx.svid, "Router resolving endpoint");

        // 2. 流控检查
        self.flow_controller.check(ctx).await?;

        // 3. 分片选择
        let shard = self
            .shard_manager
            .pick_shard(ctx.conversation_id.as_deref(), ctx.user_id.as_deref());

        // 指标：分片分布
        if let Some(ref metrics) = self.metrics {
            let svid_str = ctx.svid.to_string();
            let shard_str = shard.to_string();
            metrics
                .shard_distribution
                .with_label_values(&[&shard_str, &svid_str])
                .inc();
        }

        // 4. 读取路由表（当前每个 SVID 只有一个端点，兼容候选列表为空的情况）
        let route = repository
            .find_by_svid(&ctx.svid)
            .await
            .context("Failed to read route repository")?;

        let candidate = route.map(|r| r.endpoint().as_str().to_string());
        let candidates = candidate.into_iter().collect::<Vec<_>>();

        // 5. 负载均衡选择候选
        let endpoint = self
            .service_lb
            .pick_service(&ctx.svid, shard, &candidates)
            .ok_or_else(|| anyhow::anyhow!("No endpoint candidates for SVID {}", ctx.svid))?;

        let elapsed_ms = (Utc::now() - start).num_milliseconds() as f64;

        // 指标：路由延迟
        if let Some(ref metrics) = self.metrics {
            let svid_str = ctx.svid.to_string();
            let tenant_str = ctx
                .tenant_id
                .clone()
                .unwrap_or_else(|| "unknown".to_string());
            metrics
                .route_resolve_duration_ms
                .with_label_values(&[&svid_str, &tenant_str])
                .observe(elapsed_ms);
        }

        tracing::info!(
            svid = %ctx.svid,
            shard = shard,
            endpoint = %endpoint,
            elapsed_ms = elapsed_ms,
            time_ms = %Utc::now().timestamp_millis(),
            "✅ Router resolved endpoint"
        );

        Ok(endpoint)
    }
}
