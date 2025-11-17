#!/bin/bash
# 运行聊天室测试脚本

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  聊天室消息流程测试${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# 检查服务状态
echo -e "${YELLOW}1. 检查服务状态...${NC}"
cd "$PROJECT_ROOT"

SERVICES=(
    "signaling-online"
    "message-orchestrator"
    "storage-writer"
    "push-server"
    "access-gateway"
)

ALL_RUNNING=true
for service in "${SERVICES[@]}"; do
    pid_file="/tmp/flare-$service.pid"
    if [ -f "$pid_file" ]; then
        pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${GREEN}   ✅ $service (PID: $pid)${NC}"
        else
            echo -e "${RED}   ❌ $service (进程不存在)${NC}"
            ALL_RUNNING=false
        fi
    else
        echo -e "${RED}   ❌ $service (未启动)${NC}"
        ALL_RUNNING=false
    fi
done

if [ "$ALL_RUNNING" != true ]; then
    echo ""
    echo -e "${RED}❌ 部分服务未运行，请先启动所有服务：${NC}"
    echo -e "${GREEN}   ./scripts/start_chatroom.sh${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}✅ 所有服务运行正常${NC}"
echo ""

# 检查基础设施
echo -e "${YELLOW}2. 检查基础设施服务...${NC}"
cd "$PROJECT_ROOT/deploy"
if docker-compose ps | grep -q "Up"; then
    echo -e "${GREEN}   ✅ 基础设施服务运行中${NC}"
else
    echo -e "${RED}   ❌ 基础设施服务未运行${NC}"
    exit 1
fi

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${GREEN}✅ 测试环境准备完成！${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${YELLOW}📝 测试步骤：${NC}"
echo ""
echo "1. 启动第一个客户端（终端 1）："
echo -e "   ${GREEN}cargo run --example chatroom_client -- user1${NC}"
echo ""
echo "2. 启动第二个客户端（终端 2）："
echo -e "   ${GREEN}cargo run --example chatroom_client -- user2${NC}"
echo ""
echo "3. 启动第三个客户端（终端 3，可选）："
echo -e "   ${GREEN}cargo run --example chatroom_client -- user3${NC}"
echo ""
echo "4. 在任意客户端发送消息，验证："
echo "   - 所有在线客户端都能收到消息"
echo "   - 消息被存储到数据库"
echo ""
echo -e "${YELLOW}📋 实时查看日志：${NC}"
echo ""
echo "在另一个终端运行以下命令查看日志："
echo -e "   ${GREEN}tail -f /tmp/flare-access-gateway.log${NC}"
echo -e "   ${GREEN}tail -f /tmp/flare-message-orchestrator.log${NC}"
echo -e "   ${GREEN}tail -f /tmp/flare-push-server.log${NC}"
echo -e "   ${GREEN}tail -f /tmp/flare-storage-writer.log${NC}"
echo ""
echo -e "${YELLOW}🔍 验证消息存储：${NC}"
echo ""
echo "查询数据库验证消息已存储："
echo -e "   ${GREEN}psql -h localhost -p 25432 -U flare -d flare -c \"SELECT id, sender_id, session_id, message_type, created_at FROM messages ORDER BY created_at DESC LIMIT 10;\"${NC}"
echo ""

