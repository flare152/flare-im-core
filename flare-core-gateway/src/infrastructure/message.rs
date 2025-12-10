//! 消息服务客户端

use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tonic::{Request, Response, Status};

use flare_proto::message::message_service_client::MessageServiceClient;
use flare_proto::message::*;

use flare_server_core::discovery::ServiceClient;

/// gRPC消息服务客户端
pub struct GrpcMessageClient {
    /// 服务客户端（用于服务发现）
    service_client: Option<Arc<Mutex<ServiceClient>>>,
    /// 服务名称
    service_name: String,
    /// 直连地址（当没有服务发现时使用）
    direct_address: Option<String>,
}

impl GrpcMessageClient {
    /// 创建新的gRPC消息服务客户端
    pub fn new(service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: None,
        }
    }

    /// 使用服务客户端创建gRPC消息服务客户端
    pub fn with_service_client(service_client: ServiceClient, service_name: String) -> Self {
        Self {
            service_client: Some(Arc::new(Mutex::new(service_client))),
            service_name,
            direct_address: None,
        }
    }

    /// 使用直接地址创建gRPC消息服务客户端
    pub fn with_direct_address(direct_address: String, service_name: String) -> Self {
        Self {
            service_client: None,
            service_name,
            direct_address: Some(direct_address),
        }
    }

    /// 获取gRPC客户端
    async fn get_client(&self) -> Result<MessageServiceClient<Channel>, Status> {
        if let Some(service_client) = &self.service_client {
            let mut client = service_client.lock().await;
            let channel = client.get_channel().await.map_err(|e| {
                Status::unavailable(format!("Failed to get channel from service discovery: {}", e))
            })?;
            Ok(MessageServiceClient::new(channel))
        } else if let Some(ref address) = self.direct_address {
            let channel = Channel::from_shared(address.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid address: {}", e)))?
                .connect()
                .await
                .map_err(|e| Status::unavailable(format!("Failed to connect to {}: {}", address, e)))?;
            Ok(MessageServiceClient::new(channel))
        } else {
            // 使用服务名称进行直连（假设服务名称可以直接解析）
            let channel = Channel::from_shared(self.service_name.clone())
                .map_err(|e| Status::invalid_argument(format!("Invalid service name: {}", e)))?
                .connect()
                .await
                .map_err(|e| Status::unavailable(format!("Failed to connect to {}: {}", self.service_name, e)))?;
            Ok(MessageServiceClient::new(channel))
        }
    }

    /// 发送单条消息
    pub async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.send_message(request).await
    }

    /// 批量发送消息
    pub async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.batch_send_message(request).await
    }

    /// 发送系统消息
    pub async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.send_system_message(request).await
    }

    /// 查询会话消息
    pub async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        let mut client = self.get_client().await?;
        client.query_messages(request).await
    }

    /// 搜索消息
    pub async fn search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        let mut client = self.get_client().await?;
        client.search_messages(request).await
    }

    /// 获取单条消息
    pub async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.get_message(request).await
    }

    /// 撤回消息
    pub async fn recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.recall_message(request).await
    }

    /// 删除消息
    pub async fn delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.delete_message(request).await
    }

    /// 标记消息已读
    pub async fn mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.mark_message_read(request).await
    }

    /// 编辑消息
    pub async fn edit_message(
        &self,
        request: Request<EditMessageRequest>,
    ) -> Result<Response<EditMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.edit_message(request).await
    }

    /// 添加表情反应
    pub async fn add_reaction(
        &self,
        request: Request<AddReactionRequest>,
    ) -> Result<Response<AddReactionResponse>, Status> {
        let mut client = self.get_client().await?;
        client.add_reaction(request).await
    }

    /// 移除表情反应
    pub async fn remove_reaction(
        &self,
        request: Request<RemoveReactionRequest>,
    ) -> Result<Response<RemoveReactionResponse>, Status> {
        let mut client = self.get_client().await?;
        client.remove_reaction(request).await
    }

    /// 回复消息
    pub async fn reply_message(
        &self,
        request: Request<ReplyMessageRequest>,
    ) -> Result<Response<ReplyMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.reply_message(request).await
    }

    /// 转发消息
    pub async fn forward_message(
        &self,
        request: Request<ForwardMessageRequest>,
    ) -> Result<Response<ForwardMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.forward_message(request).await
    }

    /// 引用消息
    pub async fn quote_message(
        &self,
        request: Request<QuoteMessageRequest>,
    ) -> Result<Response<QuoteMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.quote_message(request).await
    }

    /// 在线程中回复
    pub async fn add_thread_reply(
        &self,
        request: Request<AddThreadReplyRequest>,
    ) -> Result<Response<AddThreadReplyResponse>, Status> {
        let mut client = self.get_client().await?;
        client.add_thread_reply(request).await
    }

    /// 置顶消息
    pub async fn pin_message(
        &self,
        request: Request<PinMessageRequest>,
    ) -> Result<Response<PinMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.pin_message(request).await
    }

    /// 取消置顶
    pub async fn unpin_message(
        &self,
        request: Request<UnpinMessageRequest>,
    ) -> Result<Response<UnpinMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.unpin_message(request).await
    }

    /// 收藏消息
    pub async fn favorite_message(
        &self,
        request: Request<FavoriteMessageRequest>,
    ) -> Result<Response<FavoriteMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.favorite_message(request).await
    }

    /// 取消收藏
    pub async fn unfavorite_message(
        &self,
        request: Request<UnfavoriteMessageRequest>,
    ) -> Result<Response<UnfavoriteMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.unfavorite_message(request).await
    }

    /// 标记消息
    pub async fn mark_message(
        &self,
        request: Request<MarkMessageRequest>,
    ) -> Result<Response<MarkMessageResponse>, Status> {
        let mut client = self.get_client().await?;
        client.mark_message(request).await
    }

    /// 批量标记已读
    pub async fn batch_mark_message_read(
        &self,
        request: Request<BatchMarkMessageReadRequest>,
    ) -> Result<Response<BatchMarkMessageReadResponse>, Status> {
        let mut client = self.get_client().await?;
        client.batch_mark_message_read(request).await
    }
}