//! # Simple Gateway Handler
//!
//! 简单网关处理器，仅作为代理层转发请求到后端服务
//! 职责：
//! - 媒体服务代理 (media.proto)
//! - Hook管理代理 (hooks.proto)
//! - 消息操作代理 (message.proto)
//! - 用户在线状态代理 (online.proto)
//! - 会话管理代理 (session.proto)

use std::sync::Arc;
use tonic::{Request, Response, Status};

// 媒体服务
use flare_proto::media::media_service_server::MediaService;
use flare_proto::media::*;

// Hook服务
use flare_proto::hooks::hook_service_server::HookService;
use flare_proto::hooks::*;

// 消息服务
use flare_proto::message::message_service_server::MessageService;
use flare_proto::message::*;

// 在线状态服务
use flare_proto::signaling::online::{signaling_service_server::SignalingService, user_service_server::UserService};
use flare_proto::signaling::online::*;

// 会话服务
use flare_proto::session::session_service_server::SessionService;
use flare_proto::session::*;

use crate::infrastructure::media::GrpcMediaClient;
use crate::infrastructure::hook::GrpcHookClient;
use crate::infrastructure::message::GrpcMessageClient;
use crate::infrastructure::online::GrpcOnlineClient;
use crate::infrastructure::session::GrpcSessionClient;

/// 简单网关处理器
#[derive(Clone)]
pub struct SimpleGatewayHandler {
    /// 媒体服务客户端
    media_client: Arc<GrpcMediaClient>,
    /// Hook服务客户端
    hook_client: Arc<GrpcHookClient>,
    /// 消息服务客户端
    message_client: Arc<GrpcMessageClient>,
    /// 在线状态服务客户端
    online_client: Arc<GrpcOnlineClient>,
    /// 会话服务客户端
    session_client: Arc<GrpcSessionClient>,
}

impl SimpleGatewayHandler {
    /// 创建简单网关处理器
    pub fn new(
        media_client: Arc<GrpcMediaClient>,
        hook_client: Arc<GrpcHookClient>,
        message_client: Arc<GrpcMessageClient>,
        online_client: Arc<GrpcOnlineClient>,
        session_client: Arc<GrpcSessionClient>,
    ) -> Self {
        Self {
            media_client,
            hook_client,
            message_client,
            online_client,
            session_client,
        }
    }
}

#[tonic::async_trait]
impl MediaService for SimpleGatewayHandler {
    /// 上传文件（流式）
    async fn upload_file(
        &self,
        request: Request<tonic::Streaming<UploadFileRequest>>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        // 代理到真实的媒体服务
        (*self.media_client).upload_file(request).await
    }

    /// 初始化分片上传
    async fn initiate_multipart_upload(
        &self,
        request: Request<InitiateMultipartUploadRequest>,
    ) -> Result<Response<InitiateMultipartUploadResponse>, Status> {
        self.media_client.initiate_multipart_upload(request).await
    }

    /// 上传单个分片
    async fn upload_multipart_chunk(
        &self,
        request: Request<UploadMultipartChunkRequest>,
    ) -> Result<Response<UploadMultipartChunkResponse>, Status> {
        self.media_client.upload_multipart_chunk(request).await
    }

    /// 完成分片上传
    async fn complete_multipart_upload(
        &self,
        request: Request<CompleteMultipartUploadRequest>,
    ) -> Result<Response<UploadFileResponse>, Status> {
        self.media_client.complete_multipart_upload(request).await
    }

    /// 取消分片上传
    async fn abort_multipart_upload(
        &self,
        request: Request<AbortMultipartUploadRequest>,
    ) -> Result<Response<AbortMultipartUploadResponse>, Status> {
        self.media_client.abort_multipart_upload(request).await
    }

    /// 创建媒资引用
    async fn create_reference(
        &self,
        request: Request<CreateReferenceRequest>,
    ) -> Result<Response<CreateReferenceResponse>, Status> {
        self.media_client.create_reference(request).await
    }

    /// 删除媒资引用
    async fn delete_reference(
        &self,
        request: Request<DeleteReferenceRequest>,
    ) -> Result<Response<DeleteReferenceResponse>, Status> {
        self.media_client.delete_reference(request).await
    }

    /// 列出媒资引用
    async fn list_references(
        &self,
        request: Request<ListReferencesRequest>,
    ) -> Result<Response<ListReferencesResponse>, Status> {
        self.media_client.list_references(request).await
    }

    /// 清理孤立媒资
    async fn cleanup_orphaned_assets(
        &self,
        request: Request<CleanupOrphanedAssetsRequest>,
    ) -> Result<Response<CleanupOrphanedAssetsResponse>, Status> {
        self.media_client.cleanup_orphaned_assets(request).await
    }

    /// 获取文件URL
    async fn get_file_url(
        &self,
        request: Request<GetFileUrlRequest>,
    ) -> Result<Response<GetFileUrlResponse>, Status> {
        self.media_client.get_file_url(request).await
    }

    /// 获取文件信息
    async fn get_file_info(
        &self,
        request: Request<GetFileInfoRequest>,
    ) -> Result<Response<GetFileInfoResponse>, Status> {
        self.media_client.get_file_info(request).await
    }

    /// 删除文件
    async fn delete_file(
        &self,
        request: Request<DeleteFileRequest>,
    ) -> Result<Response<DeleteFileResponse>, Status> {
        self.media_client.delete_file(request).await
    }

    /// 处理图片
    async fn process_image(
        &self,
        request: Request<ProcessImageRequest>,
    ) -> Result<Response<ProcessImageResponse>, Status> {
        self.media_client.process_image(request).await
    }

    /// 处理视频
    async fn process_video(
        &self,
        request: Request<ProcessVideoRequest>,
    ) -> Result<Response<ProcessVideoResponse>, Status> {
        self.media_client.process_video(request).await
    }

    /// 设置对象ACL
    async fn set_object_acl(
        &self,
        request: Request<SetObjectAclRequest>,
    ) -> Result<Response<SetObjectAclResponse>, Status> {
        self.media_client.set_object_acl(request).await
    }

    /// 列出对象
    async fn list_objects(
        &self,
        request: Request<ListObjectsRequest>,
    ) -> Result<Response<ListObjectsResponse>, Status> {
        self.media_client.list_objects(request).await
    }

    /// 生成上传URL
    async fn generate_upload_url(
        &self,
        request: Request<GenerateUploadUrlRequest>,
    ) -> Result<Response<GenerateUploadUrlResponse>, Status> {
        self.media_client.generate_upload_url(request).await
    }

    /// 描述存储桶
    async fn describe_bucket(
        &self,
        request: Request<DescribeBucketRequest>,
    ) -> Result<Response<DescribeBucketResponse>, Status> {
        self.media_client.describe_bucket(request).await
    }
}

#[tonic::async_trait]
impl HookService for SimpleGatewayHandler {
    /// 创建Hook配置
    async fn create_hook_config(
        &self,
        request: Request<CreateHookConfigRequest>,
    ) -> Result<Response<CreateHookConfigResponse>, Status> {
        self.hook_client.create_hook_config(request).await
    }

    /// 获取Hook配置
    async fn get_hook_config(
        &self,
        request: Request<GetHookConfigRequest>,
    ) -> Result<Response<GetHookConfigResponse>, Status> {
        self.hook_client.get_hook_config(request).await
    }

    /// 更新Hook配置
    async fn update_hook_config(
        &self,
        request: Request<UpdateHookConfigRequest>,
    ) -> Result<Response<UpdateHookConfigResponse>, Status> {
        self.hook_client.update_hook_config(request).await
    }

    /// 列出Hook配置
    async fn list_hook_configs(
        &self,
        request: Request<ListHookConfigsRequest>,
    ) -> Result<Response<ListHookConfigsResponse>, Status> {
        self.hook_client.list_hook_configs(request).await
    }

    /// 删除Hook配置
    async fn delete_hook_config(
        &self,
        request: Request<DeleteHookConfigRequest>,
    ) -> Result<Response<DeleteHookConfigResponse>, Status> {
        self.hook_client.delete_hook_config(request).await
    }

    /// 启用/禁用Hook
    async fn set_hook_status(
        &self,
        request: Request<SetHookStatusRequest>,
    ) -> Result<Response<SetHookStatusResponse>, Status> {
        self.hook_client.set_hook_status(request).await
    }

    /// 查询Hook执行统计
    async fn get_hook_statistics(
        &self,
        request: Request<GetHookStatisticsRequest>,
    ) -> Result<Response<GetHookStatisticsResponse>, Status> {
        self.hook_client.get_hook_statistics(request).await
    }

    /// 查询Hook执行历史
    async fn query_hook_executions(
        &self,
        request: Request<QueryHookExecutionsRequest>,
    ) -> Result<Response<QueryHookExecutionsResponse>, Status> {
        self.hook_client.query_hook_executions(request).await
    }
}

#[tonic::async_trait]
impl MessageService for SimpleGatewayHandler {
    /// 发送单条消息
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        self.message_client.send_message(request).await
    }

    /// 批量发送消息
    async fn batch_send_message(
        &self,
        request: Request<BatchSendMessageRequest>,
    ) -> Result<Response<BatchSendMessageResponse>, Status> {
        self.message_client.batch_send_message(request).await
    }

    /// 发送系统消息
    async fn send_system_message(
        &self,
        request: Request<SendSystemMessageRequest>,
    ) -> Result<Response<SendSystemMessageResponse>, Status> {
        self.message_client.send_system_message(request).await
    }

    /// 查询会话消息
    async fn query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        self.message_client.query_messages(request).await
    }

    /// 搜索消息
    async fn search_messages(
        &self,
        request: Request<SearchMessagesRequest>,
    ) -> Result<Response<SearchMessagesResponse>, Status> {
        self.message_client.search_messages(request).await
    }

    /// 获取单条消息
    async fn get_message(
        &self,
        request: Request<GetMessageRequest>,
    ) -> Result<Response<GetMessageResponse>, Status> {
        self.message_client.get_message(request).await
    }

    /// 撤回消息
    async fn recall_message(
        &self,
        request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        self.message_client.recall_message(request).await
    }

    /// 删除消息
    async fn delete_message(
        &self,
        request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        self.message_client.delete_message(request).await
    }

    /// 标记消息已读
    async fn mark_message_read(
        &self,
        request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        self.message_client.mark_message_read(request).await
    }

    /// 编辑消息
    async fn edit_message(
        &self,
        request: Request<EditMessageRequest>,
    ) -> Result<Response<EditMessageResponse>, Status> {
        self.message_client.edit_message(request).await
    }

    /// 添加表情反应
    async fn add_reaction(
        &self,
        request: Request<AddReactionRequest>,
    ) -> Result<Response<AddReactionResponse>, Status> {
        self.message_client.add_reaction(request).await
    }

    /// 移除表情反应
    async fn remove_reaction(
        &self,
        request: Request<RemoveReactionRequest>,
    ) -> Result<Response<RemoveReactionResponse>, Status> {
        self.message_client.remove_reaction(request).await
    }

    /// 回复消息
    async fn reply_message(
        &self,
        request: Request<ReplyMessageRequest>,
    ) -> Result<Response<ReplyMessageResponse>, Status> {
        self.message_client.reply_message(request).await
    }

    /// 转发消息
    async fn forward_message(
        &self,
        request: Request<ForwardMessageRequest>,
    ) -> Result<Response<ForwardMessageResponse>, Status> {
        self.message_client.forward_message(request).await
    }

    /// 引用消息
    async fn quote_message(
        &self,
        request: Request<QuoteMessageRequest>,
    ) -> Result<Response<QuoteMessageResponse>, Status> {
        self.message_client.quote_message(request).await
    }

    /// 在线程中回复
    async fn add_thread_reply(
        &self,
        request: Request<AddThreadReplyRequest>,
    ) -> Result<Response<AddThreadReplyResponse>, Status> {
        self.message_client.add_thread_reply(request).await
    }

    /// 置顶消息
    async fn pin_message(
        &self,
        request: Request<PinMessageRequest>,
    ) -> Result<Response<PinMessageResponse>, Status> {
        self.message_client.pin_message(request).await
    }

    /// 取消置顶
    async fn unpin_message(
        &self,
        request: Request<UnpinMessageRequest>,
    ) -> Result<Response<UnpinMessageResponse>, Status> {
        self.message_client.unpin_message(request).await
    }

    /// 收藏消息
    async fn favorite_message(
        &self,
        request: Request<FavoriteMessageRequest>,
    ) -> Result<Response<FavoriteMessageResponse>, Status> {
        self.message_client.favorite_message(request).await
    }

    /// 取消收藏
    async fn unfavorite_message(
        &self,
        request: Request<UnfavoriteMessageRequest>,
    ) -> Result<Response<UnfavoriteMessageResponse>, Status> {
        self.message_client.unfavorite_message(request).await
    }

    /// 标记消息
    async fn mark_message(
        &self,
        request: Request<MarkMessageRequest>,
    ) -> Result<Response<MarkMessageResponse>, Status> {
        self.message_client.mark_message(request).await
    }

    /// 批量标记已读
    async fn batch_mark_message_read(
        &self,
        request: Request<BatchMarkMessageReadRequest>,
    ) -> Result<Response<BatchMarkMessageReadResponse>, Status> {
        self.message_client.batch_mark_message_read(request).await
    }
}

// 定义流类型以解决编译错误
type WatchPresenceStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<flare_proto::signaling::online::PresenceEvent, Status>> + Send + Sync + 'static>>;
type SubscribeUserPresenceStream = std::pin::Pin<Box<dyn futures::Stream<Item = Result<flare_proto::signaling::online::UserPresenceEvent, Status>> + Send + Sync + 'static>>;

#[tonic::async_trait]
impl SignalingService for SimpleGatewayHandler {
    type WatchPresenceStream = WatchPresenceStream;

    /// 用户登录
    async fn login(
        &self,
        request: Request<LoginRequest>,
    ) -> Result<Response<LoginResponse>, Status> {
        self.online_client.login(request).await
    }

    /// 用户登出
    async fn logout(
        &self,
        request: Request<LogoutRequest>,
    ) -> Result<Response<LogoutResponse>, Status> {
        self.online_client.logout(request).await
    }

    /// 心跳
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        self.online_client.heartbeat(request).await
    }

    /// 获取在线状态
    async fn get_online_status(
        &self,
        request: Request<GetOnlineStatusRequest>,
    ) -> Result<Response<GetOnlineStatusResponse>, Status> {
        self.online_client.get_online_status(request).await
    }

    /// 监听在线状态变化
    async fn watch_presence(
        &self,
        request: Request<WatchPresenceRequest>,
    ) -> Result<Response<Self::WatchPresenceStream>, Status> {
        let stream = self.online_client.watch_presence(request).await?.into_inner();
        Ok(Response::new(Box::pin(stream)))
    }
}

#[tonic::async_trait]
impl UserService for SimpleGatewayHandler {
    type SubscribeUserPresenceStream = SubscribeUserPresenceStream;

    /// 查询用户在线状态
    async fn get_user_presence(
        &self,
        request: Request<GetUserPresenceRequest>,
    ) -> Result<Response<GetUserPresenceResponse>, Status> {
        self.online_client.get_user_presence(request).await
    }

    /// 批量查询在线状态
    async fn batch_get_user_presence(
        &self,
        request: Request<BatchGetUserPresenceRequest>,
    ) -> Result<Response<BatchGetUserPresenceResponse>, Status> {
        self.online_client.batch_get_user_presence(request).await
    }

    /// 订阅用户状态变化
    async fn subscribe_user_presence(
        &self,
        request: Request<SubscribeUserPresenceRequest>,
    ) -> Result<Response<Self::SubscribeUserPresenceStream>, Status> {
        let stream = self.online_client.subscribe_user_presence(request).await?.into_inner();
        Ok(Response::new(Box::pin(stream)))
    }

    /// 列出用户设备
    async fn list_user_devices(
        &self,
        request: Request<ListUserDevicesRequest>,
    ) -> Result<Response<ListUserDevicesResponse>, Status> {
        self.online_client.list_user_devices(request).await
    }

    /// 踢出设备
    async fn kick_device(
        &self,
        request: Request<KickDeviceRequest>,
    ) -> Result<Response<KickDeviceResponse>, Status> {
        self.online_client.kick_device(request).await
    }

    /// 查询设备信息
    async fn get_device(
        &self,
        request: Request<GetDeviceRequest>,
    ) -> Result<Response<GetDeviceResponse>, Status> {
        self.online_client.get_device(request).await
    }
}

#[tonic::async_trait]
impl SessionService for SimpleGatewayHandler {
    /// 会话引导
    async fn session_bootstrap(
        &self,
        request: Request<SessionBootstrapRequest>,
    ) -> Result<Response<SessionBootstrapResponse>, Status> {
        self.session_client.session_bootstrap(request).await
    }

    /// 列出会话
    async fn list_sessions(
        &self,
        request: Request<ListSessionsRequest>,
    ) -> Result<Response<ListSessionsResponse>, Status> {
        self.session_client.list_sessions(request).await
    }

    /// 同步消息
    async fn sync_messages(
        &self,
        request: Request<SyncMessagesRequest>,
    ) -> Result<Response<SyncMessagesResponse>, Status> {
        self.session_client.sync_messages(request).await
    }

    /// 会话增量同步
    async fn sync_sessions(
        &self,
        request: Request<flare_proto::common::SyncSessionsRequest>,
    ) -> Result<Response<flare_proto::common::SyncSessionsResponse>, Status> {
        self.session_client.sync_sessions(request).await
    }

    /// 会话全量恢复
    async fn get_all_sessions(
        &self,
        request: Request<flare_proto::common::SessionSyncAllRequest>,
    ) -> Result<Response<flare_proto::common::SessionSyncAllResponse>, Status> {
        self.session_client.get_all_sessions(request).await
    }

    /// 更新游标
    async fn update_cursor(
        &self,
        request: Request<UpdateCursorRequest>,
    ) -> Result<Response<UpdateCursorResponse>, Status> {
        self.session_client.update_cursor(request).await
    }

    /// 更新设备在线状态
    async fn update_presence(
        &self,
        request: Request<UpdatePresenceRequest>,
    ) -> Result<Response<UpdatePresenceResponse>, Status> {
        self.session_client.update_presence(request).await
    }

    /// 强制会话同步
    async fn force_session_sync(
        &self,
        request: Request<ForceSessionSyncRequest>,
    ) -> Result<Response<ForceSessionSyncResponse>, Status> {
        self.session_client.force_session_sync(request).await
    }

    /// 创建会话
    async fn create_session(
        &self,
        request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        self.session_client.create_session(request).await
    }

    /// 更新会话
    async fn update_session(
        &self,
        request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        self.session_client.update_session(request).await
    }

    /// 删除会话
    async fn delete_session(
        &self,
        request: Request<DeleteSessionRequest>,
    ) -> Result<Response<DeleteSessionResponse>, Status> {
        self.session_client.delete_session(request).await
    }

    /// 管理参与者
    async fn manage_participants(
        &self,
        request: Request<ManageParticipantsRequest>,
    ) -> Result<Response<ManageParticipantsResponse>, Status> {
        self.session_client.manage_participants(request).await
    }

    /// 批量确认
    async fn batch_acknowledge(
        &self,
        request: Request<BatchAcknowledgeRequest>,
    ) -> Result<Response<BatchAcknowledgeResponse>, Status> {
        self.session_client.batch_acknowledge(request).await
    }

    /// 搜索会话
    async fn search_sessions(
        &self,
        request: Request<SearchSessionsRequest>,
    ) -> Result<Response<SearchSessionsResponse>, Status> {
        self.session_client.search_sessions(request).await
    }

    /// 创建话题
    async fn create_thread(
        &self,
        request: Request<CreateThreadRequest>,
    ) -> Result<Response<CreateThreadResponse>, Status> {
        self.session_client.create_thread(request).await
    }

    /// 获取话题列表
    async fn list_threads(
        &self,
        request: Request<ListThreadsRequest>,
    ) -> Result<Response<ListThreadsResponse>, Status> {
        self.session_client.list_threads(request).await
    }

    /// 获取话题详情
    async fn get_thread(
        &self,
        request: Request<GetThreadRequest>,
    ) -> Result<Response<GetThreadResponse>, Status> {
        self.session_client.get_thread(request).await
    }

    /// 更新话题
    async fn update_thread(
        &self,
        request: Request<UpdateThreadRequest>,
    ) -> Result<Response<UpdateThreadResponse>, Status> {
        self.session_client.update_thread(request).await
    }

    /// 删除话题
    async fn delete_thread(
        &self,
        request: Request<DeleteThreadRequest>,
    ) -> Result<Response<DeleteThreadResponse>, Status> {
        self.session_client.delete_thread(request).await
    }
}


