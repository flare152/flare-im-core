#!/bin/bash
# 手动启动北京网关 (beijing-1) 用于测试
#
# 端口配置：
#   - WebSocket: 60051 (客户端连接)
#   - QUIC: 60052 (客户端连接，自动计算：60051 + 1)
#   - gRPC: 60060 (服务间调用，注册到服务注册中心)
#
# 使用方式：
#   ./scripts/start_beijing_gateway.sh
#   或者直接运行下面的命令

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_ROOT"

# 颜色输出
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  启动北京网关 (beijing-1)${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${YELLOW}📋 配置信息:${NC}"
echo "   Gateway ID:  gateway-beijing-1"
echo "   Region:      beijing"
echo "   WebSocket:   60051"
echo "   QUIC:        60052 (自动计算)"
echo "   gRPC:        60060"
echo ""
echo -e "${YELLOW}💡 提示:${NC}"
echo "   - 按 Ctrl+C 停止服务"
echo "   - 日志会直接输出到终端"
echo "   - 如需后台运行，请使用 start_multi_gateway.sh"
echo ""
echo -e "${GREEN}🚀 正在启动...${NC}"
echo ""

# 启动命令（前台运行，直接输出日志）
GATEWAY_ID="gateway-beijing-1" \
GATEWAY_REGION="beijing" \
PORT="60051" \
GRPC_PORT="60060" \
cargo run -p flare-signaling-gateway --bin flare-signaling-gateway

