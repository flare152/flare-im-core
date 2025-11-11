# Flare IM 通信核心层部署指南

> **版本**: 0.1.0  
> **用途**: 提供中间件和服务的 Docker Compose 部署配置

---

## 📋 概述

本目录包含 Flare IM 通信核心层所需的中间件和服务的 Docker Compose 配置，覆盖注册发现、消息队列、缓存、存储，以及基于 Grafana Stack（Prometheus + Loki）的可观测性能力。

---

## 🚀 快速开始

### 1. 启动所有中间件

```bash
cd deploy
docker-compose up -d
```

### 2. 检查服务状态

```bash
docker-compose ps
```

### 3. 查看日志

```bash
docker-compose logs -f [service_name]
```

---

## 📦 包含的服务

### 1. etcd (服务注册发现)

- **端口**: 22379 (客户端), 22380 (节点间通信)
- **用途**: 服务注册和发现
- **访问**: http://localhost:22379
- **数据目录**: `./data/etcd`

### 2. Redis / Dragonfly (缓存)

- **端口**: 26379
- **用途**: 缓存、在线状态存储（默认使用 Redis 单节点，亦可替换为 DragonflyDB）
- **访问**: redis://localhost:26379
- **数据目录**: `./data/redis`

### 3. PostgreSQL + TimescaleDB (时序数据库)

- **端口**: 25432
- **数据库**: flare
- **用户**: flare
- **密码**: flare123
- **访问**: postgresql://flare:flare123@localhost:25432/flare
- **数据目录**: `./data/postgres`
- **特性**: 
  - TimescaleDB 扩展已启用
  - 消息表使用超表（Hypertable）按时间分区
  - 支持时序数据查询优化
  - 支持连续聚合视图

### 4. Kafka (消息队列)

- **端口**: 29092
- **用途**: 消息队列、削峰（KRaft 模式，无需 ZooKeeper）
- **访问**: localhost:29092
- **数据目录**: `./data/kafka`

### 5. MinIO (对象存储)

- **API端口**: 29000
- **控制台端口**: 29001
- **默认用户**: minioadmin
- **默认密码**: minioadmin
- **访问**: 
  - API: http://localhost:29000
  - 控制台: http://localhost:29001
- **数据目录**: `./data/minio`

### 6. Loki (日志聚合)

- **端口**: 3100
- **用途**: 日志聚合与查询（Grafana 数据源）
- **访问**: http://localhost:3100
- **数据目录**: `./data/loki`
- **配置**: `./loki-config.yml`（默认启用本地文件存储）

### 7. Prometheus (监控)

- **端口**: 29090
- **用途**: 指标收集和监控
- **访问**: http://localhost:29090
- **数据目录**: `./data/prometheus`

### 8. Grafana (可视化 & 告警)

- **端口**: 23000
- **用途**: 监控、日志、追踪统一可视化，以及告警管理
- **访问**: http://localhost:23000
- **默认用户**: admin / admin
- **数据目录**: `./data/grafana`

> 📌 **扩展提示**：若后续需要全文检索或分布式追踪能力，可追加部署 OpenSearch、OpenSearch Dashboards、Tempo 等组件，并在 Grafana 中新增相应数据源。

---

## 🔧 配置说明

### 环境变量

可以通过环境变量配置各个服务：

```bash
# etcd
ETCD_DATA_DIR=/etcd-data

# Redis
REDIS_PASSWORD=your_password

# PostgreSQL
POSTGRES_USER=flare
POSTGRES_PASSWORD=flare123
POSTGRES_DB=flare

# MinIO
MINIO_ROOT_USER=minioadmin
MINIO_ROOT_PASSWORD=minioadmin
```

### 存储配置

数据持久化存储在本地目录 `./data/` 下：

- `./data/etcd`: etcd 数据
- `./data/redis`: Redis 数据
- `./data/postgres`: PostgreSQL 数据
- `./data/kafka`: Kafka 数据
- `./data/minio`: MinIO 数据
- `./data/loki`: Loki 数据
- `./data/prometheus`: Prometheus 数据
- `./data/grafana`: Grafana 数据

> 💡 **提示**: 首次启动前，建议创建数据目录：
> ```bash
> mkdir -p data/{etcd,redis,postgres,kafka,minio,loki,prometheus,grafana}
> ```

---

## 📝 使用示例

### 1. 启动服务

```bash
# 启动所有服务
docker-compose up -d

# 启动特定服务
docker-compose up -d redis postgres
```

### 2. 停止服务

```bash
# 停止所有服务
docker-compose down

# 停止并删除数据
docker-compose down -v
```

### 3. 查看日志

```bash
# 查看所有日志
docker-compose logs -f

# 查看特定服务日志
docker-compose logs -f kafka
```

### 4. 访问 MinIO 控制台

1. 打开浏览器访问 http://localhost:29001
2. 使用默认账号登录：minioadmin / minioadmin
3. 创建 bucket：flare-media（已自动创建）

---

## 🔗 服务连接信息

### 开发环境连接

```toml
# etcd
etcd_endpoints = ["http://localhost:22379"]

# Redis
redis_url = "redis://localhost:26379"

# PostgreSQL + TimescaleDB
postgres_url = "postgresql://flare:flare123@localhost:25432/flare"
# TimescaleDB 扩展已自动启用，消息表已转换为超表（Hypertable）

# Kafka
kafka_brokers = ["localhost:29092"]

# MinIO
minio_endpoint = "http://localhost:29000"
minio_access_key = "minioadmin"
minio_secret_key = "minioadmin"
minio_bucket = "flare-media"

# Loki
loki_url = "http://localhost:3100"

# Prometheus
prometheus_url = "http://localhost:29090"

# Grafana
grafana_url = "http://localhost:23000"
```

---

## 📚 相关文档

- [部署指南](../doc/DEPLOYMENT_GUIDE.md)
- [项目结构](../doc/PROJECT_STRUCTURE.md)
- [集成指南](../doc/INTEGRATION_GUIDE.md)
- [TimescaleDB 使用指南](./TIMESCALEDB_GUIDE.md) - TimescaleDB 详细配置和使用

---

**文档维护**: Flare IM Architecture Team  
**最后更新**: 2025-01-XX  
**版本**: 0.1.0

