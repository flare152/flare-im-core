#!/bin/bash
# 运行 flare-session 服务的集成测试
#
# 使用方式：
#   ./run_tests.sh [manual|auto]
#
# - manual: 手动启动服务（默认）
# - auto: 自动启动服务并运行测试

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 颜色输出函数
print_step() {
    echo -e "\033[1;34m==> $1\033[0m"
}

print_info() {
    echo -e "\033[1;32m✓ $1\033[0m"
}

print_warn() {
    echo -e "\033[1;33m⚠ $1\033[0m"
}

print_error() {
    echo -e "\033[1;31m✗ $1\033[0m"
}

# 检查端口是否被占用
check_port() {
    local port=$1
    if lsof -Pi :$port -sTCP:LISTEN -t >/dev/null 2>&1; then
        return 0  # 端口已被占用
    else
        return 1  # 端口未被占用
    fi
}

# 等待服务可用
wait_for_service() {
    local port=$1
    local service_name=$2
    local max_retries=30
    local retry=0

    print_step "Waiting for $service_name to be ready on port $port..."
    while [ $retry -lt $max_retries ]; do
        if check_port $port; then
            # 尝试连接服务
            if nc -z localhost $port 2>/dev/null || curl -s http://localhost:$port >/dev/null 2>&1; then
                print_info "$service_name is ready"
                return 0
            fi
        fi
        retry=$((retry + 1))
        sleep 1
    done

    print_error "$service_name failed to start on port $port"
    return 1
}

# 清理函数
cleanup() {
    if [ -n "$SESSION_PID" ]; then
        print_step "Stopping flare-session service (PID: $SESSION_PID)..."
        kill $SESSION_PID 2>/dev/null || true
        wait $SESSION_PID 2>/dev/null || true
        print_info "flare-session service stopped"
    fi
}

# 设置退出时清理
trap cleanup EXIT INT TERM

MODE=${1:-manual}
SESSION_PORT=50090
SESSION_PID=""

if [ "$MODE" = "auto" ]; then
    print_step "Auto mode: Starting flare-session service..."
    
    if check_port $SESSION_PORT; then
        print_warn "Port $SESSION_PORT is already in use, skipping service startup"
        SESSION_PID=""  # 不管理这个进程
    else
        # 在后台启动服务
        RUST_LOG=info SERVER_PORT=$SESSION_PORT cargo run --bin flare-session > /tmp/flare-session.log 2>&1 &
        SESSION_PID=$!
        print_info "Started flare-session (PID: $SESSION_PID)"
        
        # 等待服务启动
        if ! wait_for_service $SESSION_PORT "flare-session"; then
            print_error "Failed to start flare-session service"
            print_error "Log output:"
            cat /tmp/flare-session.log
            exit 1
        fi
    fi
elif [ "$MODE" = "manual" ]; then
    print_step "Manual mode: Please start flare-session service manually"
    print_info "Run: cargo run --bin flare-session"
    print_info "Or: make -C .. run-session"
    print_info "Waiting 5 seconds for you to start the service..."
    sleep 5
    
    if ! check_port $SESSION_PORT; then
        print_error "Service is not running on port $SESSION_PORT"
        print_error "Please start the service first"
        exit 1
    fi
else
    print_error "Invalid mode: $MODE (use 'manual' or 'auto')"
    exit 1
fi

# 运行测试
print_step "Running integration tests..."
cargo test --test session_service_test -- --nocapture --test-threads=1
TEST_RESULT=$?

if [ $TEST_RESULT -eq 0 ]; then
    print_info "✅ All tests passed!"
else
    print_error "❌ Some tests failed!"
    if [ -n "$SESSION_PID" ]; then
        print_error "Service log output:"
        cat /tmp/flare-session.log
    fi
fi

exit $TEST_RESULT

