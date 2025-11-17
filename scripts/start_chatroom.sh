#!/bin/bash
# 启动聊天室所需的所有服务

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEPLOY_DIR="$PROJECT_ROOT/deploy"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  Flare IM Core 聊天室启动脚本${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# 检查基础设施服务是否运行（仅检查，不启动）
echo -e "${YELLOW}📦 检查基础设施服务状态...${NC}"
check_service() {
    local service=$1
    local port=$2
    
    if nc -z localhost $port 2>/dev/null; then
        echo -e "${GREEN}   ✓ $service 已就绪 (端口 $port)${NC}"
        return 0
    else
        echo -e "${RED}   ✗ $service 未运行 (端口 $port)${NC}"
        return 1
    fi
}

check_service "Redis" "26379"
check_service "PostgreSQL" "25432"
check_service "Kafka" "29092"

echo ""
echo -e "${YELLOW}💡 提示: 如需启动基础设施服务，请运行:${NC}"
echo "   ${BLUE}cd deploy && docker-compose up -d${NC}"
echo ""
echo -e "${YELLOW}🔍 检查并停止旧进程...${NC}"
# 检查并停止可能存在的旧进程
for service in "signaling-online" "message-orchestrator" "storage-writer" "push-server" "access-gateway"; do
    pid_file="/tmp/flare-$service.pid"
    if [ -f "$pid_file" ]; then
        pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${YELLOW}   停止旧的 $service 进程 (PID: $pid)...${NC}"
            kill "$pid" 2>/dev/null || true
            sleep 1
            if ps -p "$pid" > /dev/null 2>&1; then
                kill -9 "$pid" 2>/dev/null || true
            fi
            rm -f "$pid_file"
        else
            rm -f "$pid_file"
        fi
    fi
    # 额外检查：通过进程名查找并停止（处理 PID 文件丢失的情况）
    pkill -f "target/debug/flare-$service" 2>/dev/null || true
done
sleep 1
echo -e "${GREEN}   ✓ 旧进程清理完成${NC}"
echo ""

echo -e "${GREEN}🚀 启动 Flare IM Core 服务...${NC}"
cd "$PROJECT_ROOT"

# 定义服务启动顺序
SERVICES=(
    "signaling-online"
    "message-orchestrator"
    "storage-writer"
    "push-server"
    "access-gateway"
)

# 启动服务（后台运行）
for service in "${SERVICES[@]}"; do
    echo -e "${YELLOW}   启动 $service...${NC}"
    
    # 根据服务名称构建包名和二进制名称
    case "$service" in
        "signaling-online")
            PACKAGE="flare-signaling-online"
            BIN_NAME="flare-signaling-online"
            ENV_VARS=""
            ;;
        "message-orchestrator")
            PACKAGE="flare-message-orchestrator"
            BIN_NAME="flare-message-orchestrator"
            ENV_VARS=""
            ;;
        "storage-writer")
            PACKAGE="flare-storage-writer"
            BIN_NAME="flare-storage-writer"
            ENV_VARS=""
            ;;
        "push-server")
            PACKAGE="flare-push-server"
            BIN_NAME="flare-push-server"
            ENV_VARS=""
            ;;
        "access-gateway")
            PACKAGE="flare-access-gateway"
            BIN_NAME="flare-access-gateway"
            # 支持多网关部署：通过环境变量配置 gateway_id 和 region
            # 示例：GATEWAY_ID=gateway-beijing-1 GATEWAY_REGION=beijing ./scripts/start_chatroom.sh
            ENV_VARS=""
            if [ -n "$GATEWAY_ID" ]; then
                ENV_VARS="GATEWAY_ID=$GATEWAY_ID "
                echo -e "${BLUE}     使用配置的 Gateway ID: $GATEWAY_ID${NC}"
            fi
            if [ -n "$GATEWAY_REGION" ]; then
                ENV_VARS="${ENV_VARS}GATEWAY_REGION=$GATEWAY_REGION "
                echo -e "${BLUE}     使用配置的 Region: $GATEWAY_REGION${NC}"
            fi
            ;;
        *)
            echo -e "${RED}   ✗ 未知服务: $service${NC}"
            continue
            ;;
    esac
    
    # 启动服务（使用 -p 指定包名，支持环境变量）
    if [ -n "$ENV_VARS" ]; then
        eval "$ENV_VARS cargo run -p $PACKAGE --bin $BIN_NAME > /tmp/flare-$service.log 2>&1 &"
    else
        cargo run -p "$PACKAGE" --bin "$BIN_NAME" > /tmp/flare-$service.log 2>&1 &
    fi
    echo $! > /tmp/flare-$service.pid
    sleep 3
done

# 等待服务启动
echo ""
echo -e "${YELLOW}⏳ 等待服务启动...${NC}"
sleep 10

# 检查服务是否运行
check_process() {
    local service=$1
    local pid_file="/tmp/flare-$service.pid"
    
    if [ -f "$pid_file" ]; then
        local pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${GREEN}   ✓ $service 正在运行 (PID: $pid)${NC}"
            return 0
        else
            echo -e "${RED}   ✗ $service 启动失败${NC}"
            echo -e "${YELLOW}   查看日志: tail -f /tmp/flare-$service.log${NC}"
            return 1
        fi
    else
        echo -e "${RED}   ✗ $service PID 文件不存在${NC}"
        return 1
    fi
}

echo ""
echo -e "${GREEN}📊 服务状态检查:${NC}"
for service in "${SERVICES[@]}"; do
    check_process "$service"
done

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${GREEN}✅ 聊天室服务启动完成！${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${YELLOW}📝 使用说明:${NC}"
echo ""
echo "1. 启动第一个客户端:"
echo "   ${GREEN}cargo run --example chatroom_client -- user1${NC}"
echo ""
echo "2. 启动第二个客户端（新终端）:"
echo "   ${GREEN}cargo run --example chatroom_client -- user2${NC}"
echo ""
echo "3. 启动更多客户端:"
echo "   ${GREEN}cargo run --example chatroom_client -- user3${NC}"
echo ""
echo -e "${YELLOW}🌍 多网关部署（跨地区路由）:${NC}"
echo ""
echo "启动多个 Access Gateway 实例（不同地区）:"
echo "   ${BLUE}# 北京网关${NC}"
echo "   ${GREEN}GATEWAY_ID=gateway-beijing-1 GATEWAY_REGION=beijing ./scripts/start_chatroom.sh${NC}"
echo ""
echo "   ${BLUE}# 上海网关（新终端）${NC}"
echo "   ${GREEN}GATEWAY_ID=gateway-shanghai-1 GATEWAY_REGION=shanghai ./scripts/start_chatroom.sh${NC}"
echo ""
echo "客户端连接到指定网关:"
echo "   ${GREEN}NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1${NC}"
echo "   ${GREEN}NEGOTIATION_HOST=localhost:60052 cargo run --example chatroom_client -- user2${NC}"
echo ""
echo -e "${YELLOW}📋 服务日志:${NC}"
echo "   - Access Gateway: tail -f /tmp/flare-access-gateway.log"
echo "   - Message Orchestrator: tail -f /tmp/flare-message-orchestrator.log"
echo "   - Push Server: tail -f /tmp/flare-push-server.log"
echo ""
echo -e "${YELLOW}🛑 停止服务:${NC}"
echo "   ${RED}./scripts/stop_chatroom.sh${NC}"
echo ""

