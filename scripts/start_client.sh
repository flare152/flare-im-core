#!/bin/bash
# 启动聊天室客户端

set -e

# 颜色输出
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# 获取用户ID（命令行参数或默认值）
USER_ID=${1:-user1}

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  启动聊天室客户端${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""
echo -e "${YELLOW}用户ID: ${GREEN}${USER_ID}${NC}"
echo -e "${YELLOW}服务器: ${GREEN}localhost:60051${NC}"
echo ""
echo -e "${YELLOW}使用说明:${NC}"
echo "  - 输入消息后按回车发送"
echo "  - 输入 'quit' 或 'exit' 退出"
echo "  - 输入 '/userid' 查看当前用户ID"
echo "  - 输入 '/help' 查看帮助"
echo ""
echo -e "${BLUE}========================================${NC}"
echo ""

cd "$PROJECT_ROOT"
cargo run --example chatroom_client -- "$USER_ID"

