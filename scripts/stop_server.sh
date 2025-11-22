#!/bin/bash
# 停止 Flare IM Core 所有服务模块
#
# 使用方法:
#   ./scripts/stop_service.sh [single|multi]
#   - single: 停止单网关模式（默认，仅停止单个 access-gateway 实例）
#   - multi:  停止多网关模式（停止多个 access-gateway 实例）
#
# 注意：如果不指定参数，会尝试停止所有可能的 access-gateway 实例
#      access-gateway 是 Signaling Gateway 服务，位于 flare-signaling/gateway

set -e

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
LOGS_DIR="$PROJECT_ROOT/logs"

# 解析参数（可选）
GATEWAY_MODE="${1:-auto}"  # 默认自动检测模式

if [ "$GATEWAY_MODE" != "single" ] && [ "$GATEWAY_MODE" != "multi" ] && [ "$GATEWAY_MODE" != "auto" ]; then
    echo -e "${RED}错误: 无效的参数 '$GATEWAY_MODE'${NC}"
    echo "使用方法: $0 [single|multi|auto]"
    echo "  - single: 仅停止单网关实例（默认 access-gateway.pid）"
    echo "  - multi:  仅停止多网关实例（*-access-gateway-*.pid）"
    echo "  - auto:   自动检测并停止所有 access-gateway 实例（默认）"
    exit 1
fi

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  停止 Flare IM Core 所有服务${NC}"
echo -e "${BLUE}========================================${NC}"
echo -e "${YELLOW}📁 日志目录: $LOGS_DIR${NC}"
echo -e "${YELLOW}🚪 停止模式: $GATEWAY_MODE${NC}"
echo ""

# 定义所有核心服务（包含 Makefile 中所有 run-* 服务）
CORE_SERVICES=(
    "signaling-online"
    "signaling-route"
    "hook-engine"
    "session"
    "message-orchestrator"
    "storage-writer"
    "storage-reader"
    "push-proxy"
    "push-server"
    "push-worker"
    "media"
    "core-gateway"
)

# 定义多网关配置（与 start_server.sh 保持一致，使用普通数组兼容 bash 3.x）
# 格式: gateway_key:region:gateway_id:ws_port:grpc_port
GATEWAYS=(
    "beijing-1:beijing:gateway-beijing-1:60051:60060"
    "shanghai-1:shanghai:gateway-shanghai-1:60070:60080"
)

# 停止服务函数
stop_service() {
    local service=$1
    local pid_file="$LOGS_DIR/flare-$service.pid"
    
    if [ -f "$pid_file" ]; then
        local pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${YELLOW}   停止 $service (PID: $pid)...${NC}"
            kill "$pid" 2>/dev/null || true
            sleep 1
            # 如果进程仍在运行，强制终止
            if ps -p "$pid" > /dev/null 2>&1; then
                echo -e "${YELLOW}     强制终止 $service...${NC}"
                kill -9 "$pid" 2>/dev/null || true
                sleep 1
            fi
            rm -f "$pid_file"
            echo -e "${GREEN}      ✓ $service 已停止${NC}"
            return 0
        else
            echo -e "${YELLOW}   $service 进程不存在 (PID: $pid)${NC}"
            rm -f "$pid_file"
            return 1
        fi
    else
        # 如果没有 PID 文件，尝试通过进程名查找并停止
        if pgrep -f "target/debug/flare-$service" > /dev/null 2>&1; then
            echo -e "${YELLOW}   通过进程名停止 $service...${NC}"
            pkill -f "target/debug/flare-$service" 2>/dev/null || true
            sleep 1
            # 如果仍在运行，强制终止
            if pgrep -f "target/debug/flare-$service" > /dev/null 2>&1; then
                pkill -9 -f "target/debug/flare-$service" 2>/dev/null || true
            fi
            echo -e "${GREEN}      ✓ $service 已停止${NC}"
            return 0
        else
            echo -e "${YELLOW}   $service 未运行${NC}"
            return 1
        fi
    fi
}

# 停止所有核心服务
echo -e "${YELLOW}🛑 停止核心服务...${NC}"
for service in "${CORE_SERVICES[@]}"; do
    stop_service "$service"
done

echo ""

# 停止 Access Gateway (Signaling Gateway 服务，位于 flare-signaling/gateway)（根据模式选择）
if [ "$GATEWAY_MODE" == "single" ] || [ "$GATEWAY_MODE" == "auto" ]; then
    # 停止单网关实例
    echo -e "${YELLOW}🛑 停止单网关实例...${NC}"
    pid_file="$LOGS_DIR/flare-access-gateway.pid"
    if [ -f "$pid_file" ]; then
        pid=$(cat "$pid_file")
        if ps -p "$pid" > /dev/null 2>&1; then
            echo -e "${YELLOW}   停止 access-gateway (PID: $pid)...${NC}"
            kill "$pid" 2>/dev/null || true
            sleep 1
            if ps -p "$pid" > /dev/null 2>&1; then
                kill -9 "$pid" 2>/dev/null || true
            fi
            rm -f "$pid_file"
            echo -e "${GREEN}      ✓ access-gateway 已停止${NC}"
        else
            rm -f "$pid_file"
            echo -e "${YELLOW}      access-gateway 进程不存在${NC}"
        fi
    else
        echo -e "${YELLOW}      access-gateway PID 文件不存在${NC}"
    fi
    echo ""
fi

if [ "$GATEWAY_MODE" == "multi" ] || [ "$GATEWAY_MODE" == "auto" ]; then
    # 停止多网关实例 (Signaling Gateway 服务，位于 flare-signaling/gateway)
    echo -e "${YELLOW}🛑 停止多网关实例...${NC}"
    for gateway_config in "${GATEWAYS[@]}"; do
        IFS=':' read -r gateway_key region gateway_id ws_port grpc_port <<< "$gateway_config"
        pid_file="$LOGS_DIR/flare-access-gateway-$gateway_key.pid"
        
        if [ -f "$pid_file" ]; then
            pid=$(cat "$pid_file")
            if ps -p "$pid" > /dev/null 2>&1; then
                echo -e "${YELLOW}   停止 $gateway_key (PID: $pid)...${NC}"
                kill "$pid" 2>/dev/null || true
                sleep 1
                if ps -p "$pid" > /dev/null 2>&1; then
                    kill -9 "$pid" 2>/dev/null || true
                fi
                rm -f "$pid_file"
                echo -e "${GREEN}      ✓ $gateway_key 已停止${NC}"
            else
                rm -f "$pid_file"
                echo -e "${YELLOW}      $gateway_key 进程不存在${NC}"
            fi
        else
            echo -e "${YELLOW}      $gateway_key PID 文件不存在${NC}"
        fi
    done
    echo ""
fi

echo ""
echo -e "${BLUE}========================================${NC}"
echo -e "${GREEN}✅ 所有服务已停止${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# 等待进程完全停止
echo -e "${YELLOW}⏳ 等待进程完全停止...${NC}"
sleep 2

# 调用检查脚本验证所有服务已停止
echo ""
"$SCRIPT_DIR/check_services.sh"
CHECK_RESULT=$?

echo ""
echo -e "${YELLOW}💡 提示:${NC}"
echo "   - 基础设施服务（Redis、PostgreSQL、Kafka、Consul）仍在运行"
echo "   - 如需停止基础设施服务，请运行:"
echo "     ${BLUE}cd deploy && docker-compose down${NC}"
echo ""

# 检查脚本返回非零是正常的（因为服务已停止），不以此作为退出码
exit 0

