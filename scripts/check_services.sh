#!/bin/bash
# 检查所有服务是否正常运行

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  检查 Flare IM Core 服务状态${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# 检查 Docker 服务
echo -e "${YELLOW}📦 检查 Docker 服务...${NC}"
if ! docker info > /dev/null 2>&1; then
    echo -e "${RED}❌ Docker 未运行${NC}"
    exit 1
fi

# 检查基础设施服务
echo -e "${YELLOW}📦 检查基础设施服务...${NC}"
cd "$(dirname "$0")/../deploy"

if docker-compose ps | grep -q "Up"; then
    echo -e "${GREEN}✅ 基础设施服务运行中${NC}"
    docker-compose ps | grep -E "Up|Exit" | head -10
else
    echo -e "${RED}❌ 基础设施服务未运行${NC}"
    echo -e "${YELLOW}   提示: 运行 'cd deploy && docker-compose up -d' 启动基础设施${NC}"
fi

echo ""

# 检查 Flare IM Core 服务
echo -e "${YELLOW}🚀 检查 Flare IM Core 服务...${NC}"

SERVICES=(
    "signaling-online:50061"
    "message-orchestrator:50081"
    "storage-writer:50071"
    "push-server:50091"
    "access-gateway:60051"
)

ALL_RUNNING=true

for service_port in "${SERVICES[@]}"; do
    IFS=':' read -r service port <<< "$service_port"
    pid_file="/tmp/flare-$service.pid"
    
    if [ -f "$pid_file" ]; then
        pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${GREEN}✅ $service (PID: $pid, Port: $port)${NC}"
        else
            echo -e "${RED}❌ $service (PID文件存在但进程不存在)${NC}"
            ALL_RUNNING=false
        fi
    else
        echo -e "${RED}❌ $service (未启动)${NC}"
        ALL_RUNNING=false
    fi
done

echo ""

if [ "$ALL_RUNNING" = true ]; then
    echo -e "${GREEN}✅ 所有服务运行正常${NC}"
    exit 0
else
    echo -e "${YELLOW}⚠️  部分服务未运行${NC}"
    echo -e "${YELLOW}   提示: 运行 './scripts/start_chatroom.sh' 启动所有服务${NC}"
    exit 1
fi

