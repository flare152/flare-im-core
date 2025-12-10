#!/bin/bash

echo "=========================================="
echo "Flare IM 一对一聊天测试"
echo "=========================================="

echo "正在启动两个聊天客户端..."
echo "客户端1: user1 -> user2"
echo "客户端2: user2 -> user1"
echo ""

echo "请手动在两个终端中分别输入以下命令:"
echo ""
echo "终端1 (user1 -> user2):"
echo "cd /Users/hg/workspace/flare/flare-im/flare-im-core && NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1 user2"
echo ""
echo "终端2 (user2 -> user1):"
echo "cd /Users/hg/workspace/flare/flare-im/flare-im-core && NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user2 user1"
echo ""

echo "测试步骤:"
echo "1. 等待两个客户端都显示连接成功的消息"
echo "2. 在终端1中输入: Hello from user1!"
echo "3. 检查终端2是否收到消息"
echo "4. 在终端2中输入: Hello from user2!"
echo "5. 检查终端1是否收到消息"
echo ""

echo "预期结果:"
echo "- 两个客户端都能成功连接到服务器"
echo "- 消息能够正确地从发送方传递到接收方"
echo "- 消息显示格式为: [发送方 ➡ 接收方]: 消息内容"
echo ""

echo "注意: 消息现在是点对点发送，不会广播到聊天室"