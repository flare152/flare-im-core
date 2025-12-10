#!/bin/bash

# 启动第一个客户端 (user1 -> user2)
echo "启动第一个客户端 (user1 -> user2)"
osascript -e 'tell app "Terminal" to do script "cd /Users/hg/workspace/flare/flare-im/flare-im-core && NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user1 user2"'

sleep 3

# 启动第二个客户端 (user2 -> user1)
echo "启动第二个客户端 (user2 -> user1)"
osascript -e 'tell app "Terminal" to do script "cd /Users/hg/workspace/flare/flare-im/flare-im-core && NEGOTIATION_HOST=localhost:60051 cargo run --example chatroom_client -- user2 user1"'

sleep 3

echo "两个客户端已启动，请在终端中手动发送消息进行测试"
echo "在第一个终端中输入消息发送给user2，在第二个终端中输入消息发送给user1"