//! Access Gateway gRPC 服务处理器
//!
//! 直接实现 gRPC 服务 trait，无需 server 包装

use std::sync::Arc;

use crate::application::handlers::{
    BatchPushMessageCommand, PushMessageCommand, PushMessageService,
};
use crate::application::handlers::{ConnectionQueryService, QueryUserConnectionsQuery};
use flare_proto::access_gateway::access_gateway_server::AccessGateway;
use flare_proto::access_gateway::{
    BatchPushMessageRequest, BatchPushMessageResponse, PushAckRequest, PushCustomRequest,
    PushMessageRequest, PushMessageResponse, QueryUserConnectionsRequest,
    QueryUserConnectionsResponse,
};
// 注意：SignalingService 已移除，由 flare-signaling/online 服务实现
// Gateway 只提供 AccessGateway 服务
use tonic::{Request, Response, Status};
use tracing::{debug, info, warn};

/// AccessGateway gRPC 处理器
///
/// 提供业务系统推送消息给客户端的 gRPC 接口
#[derive(Clone)]
pub struct AccessGatewayHandler {
    push_service: Arc<PushMessageService>,
    connection_query_service: Arc<ConnectionQueryService>,
    subscription_service: Arc<crate::domain::service::SubscriptionService>,
    connection_handler: Arc<crate::interface::handler::LongConnectionHandler>,
}
impl AccessGatewayHandler {
    pub fn new(
        push_service: Arc<PushMessageService>,
        connection_query_service: Arc<ConnectionQueryService>,
        subscription_service: Arc<crate::domain::service::SubscriptionService>,
        connection_handler: Arc<crate::interface::handler::LongConnectionHandler>,
    ) -> Self {
        Self {
            push_service,
            connection_query_service,
            subscription_service,
            connection_handler,
        }
    }
}
#[tonic::async_trait]
impl AccessGateway for AccessGatewayHandler {
    async fn push_message(
        &self,
        request: Request<PushMessageRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        info!("PushMessage request: {} users", req.target_user_ids.len());

        let response = self
            .push_service
            .handle_push_message(PushMessageCommand { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to push message");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }

    async fn batch_push_message(
        &self,
        request: Request<BatchPushMessageRequest>,
    ) -> Result<Response<BatchPushMessageResponse>, Status> {
        let req = request.into_inner();
        info!("BatchPushMessage request: {} tasks", req.pushes.len());

        let response = self
            .push_service
            .handle_batch_push_message(BatchPushMessageCommand { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to batch push message");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }

    async fn query_user_connections(
        &self,
        request: Request<QueryUserConnectionsRequest>,
    ) -> Result<Response<QueryUserConnectionsResponse>, Status> {
        let req = request.into_inner();
        info!("QueryUserConnections request: {} users", req.user_ids.len());

        let response = self
            .connection_query_service
            .handle_query_user_connections(QueryUserConnectionsQuery { request: req })
            .await
            .map_err(|e| {
                tracing::error!(?e, "Failed to query user connections");
                Status::internal(e.to_string())
            })?;

        Ok(Response::new(response))
    }

    async fn push_ack(
        &self,
        request: Request<PushAckRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        let ack = req
            .ack
            .ok_or_else(|| Status::invalid_argument("ack is required"))?;

        info!(
            "PushAck request: {} users, message_id: {}",
            req.target_user_ids.len(),
            ack.message_id
        );

        // 实现 ACK 推送逻辑
        // 将 ACK 封装为 ServerPacket 推送给目标用户
        let mut push_results = Vec::new();

        // 为每个目标用户构建推送结果
        for user_id in &req.target_user_ids {
            // 构建 ACK 数据包
            let ack_packet = flare_proto::common::ServerPacket {
                payload: Some(flare_proto::common::server_packet::Payload::SendAck(
                    flare_proto::common::SendEnvelopeAck {
                        message_id: ack.message_id.clone(),
                        status: if ack.status == flare_proto::common::AckStatus::Success as i32 {
                            flare_proto::common::AckStatus::Success as i32
                        } else {
                            flare_proto::common::AckStatus::Failed as i32
                        },
                        error_code: ack.error_code,
                        error_message: ack.error_message.clone(),
                    },
                )),
            };

            // 通过连接管理器推送 ACK 数据包
            match self
                .connection_handler
                .push_packet_to_user(user_id, &ack_packet)
                .await
            {
                Ok(_) => {
                    push_results.push(flare_proto::access_gateway::PushResult {
                        user_id: user_id.clone(),
                        status: flare_proto::access_gateway::PushStatus::Success as i32,
                        success_count: 1,
                        failure_count: 0,
                        error_message: String::new(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });

                    tracing::debug!(
                        message_id = %ack.message_id,
                        user_id = %user_id,
                        "ACK pushed successfully"
                    );
                }
                Err(e) => {
                    push_results.push(flare_proto::access_gateway::PushResult {
                        user_id: user_id.clone(),
                        status: flare_proto::access_gateway::PushStatus::Failed as i32,
                        success_count: 0,
                        failure_count: 1,
                        error_message: format!("Failed to push ACK: {}", e),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });

                    tracing::warn!(
                        error = %e,
                        message_id = %ack.message_id,
                        user_id = %user_id,
                        "Failed to push ACK"
                    );
                }
            }
        }

        let success_count = push_results
            .iter()
            .filter(|r| r.status == flare_proto::access_gateway::PushStatus::Success as i32)
            .count() as i32;
        let failure_count = push_results
            .iter()
            .filter(|r| r.status == flare_proto::access_gateway::PushStatus::Failed as i32)
            .count() as i32;

        Ok(Response::new(PushMessageResponse {
            request_id: req.request_id,
            results: push_results,
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: format!(
                    "ACK push completed: {} success, {} failures",
                    success_count, failure_count
                ),
                details: vec![],
                context: None,
            }),
            statistics: Some(flare_proto::access_gateway::PushStatistics {
                total_users: req.target_user_ids.len() as i32,
                online_users: success_count,
                offline_users: failure_count,
                success_count,
                failure_count,
            }),
        }))
    }

    async fn push_custom(
        &self,
        request: Request<PushCustomRequest>,
    ) -> Result<Response<PushMessageResponse>, Status> {
        let req = request.into_inner();
        let custom = req
            .custom
            .ok_or_else(|| Status::invalid_argument("custom data is required"))?;

        info!(
            "PushCustom request: {} users, type: {}",
            req.target_user_ids.len(),
            custom.r#type
        );

        // 实现自定义数据推送逻辑
        // 将 CustomPushData 封装为 ServerPacket 推送给目标用户
        let mut push_results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        // 为每个目标用户构建并推送自定义数据包
        for user_id in &req.target_user_ids {
            // 构建 ServerPacket，包含自定义推送数据
            let packet = flare_proto::common::ServerPacket {
                payload: Some(flare_proto::common::server_packet::Payload::CustomPushData(
                    custom.clone(),
                )),
            };

            // 推送数据包到用户
            match self
                .connection_handler
                .push_packet_to_user(user_id, &packet)
                .await
            {
                Ok(_) => {
                    success_count += 1;
                    push_results.push(flare_proto::access_gateway::PushResult {
                        user_id: user_id.clone(),
                        status: flare_proto::access_gateway::PushStatus::Success as i32,
                        success_count: 1,
                        failure_count: 0,
                        error_message: String::new(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });

                    debug!(
                        user_id = %user_id,
                        custom_type = %custom.r#type,
                        "Custom push data sent successfully"
                    );
                }
                Err(err) => {
                    failure_count += 1;
                    push_results.push(flare_proto::access_gateway::PushResult {
                        user_id: user_id.clone(),
                        status: flare_proto::access_gateway::PushStatus::Failed as i32,
                        success_count: 0,
                        failure_count: 1,
                        error_message: err.to_string(),
                        pushed_at: Some(prost_types::Timestamp {
                            seconds: chrono::Utc::now().timestamp(),
                            nanos: 0,
                        }),
                    });

                    warn!(
                        error = %err,
                        user_id = %user_id,
                        custom_type = %custom.r#type,
                        "Failed to push custom data to user"
                    );
                }
            }
        }

        Ok(Response::new(PushMessageResponse {
            request_id: req.request_id,
            results: push_results,
            status: Some(flare_proto::RpcStatus {
                code: flare_proto::common::ErrorCode::Ok as i32,
                message: format!(
                    "Custom push completed: {} success, {} failures",
                    success_count, failure_count
                ),
                details: vec![],
                context: None,
            }),
            statistics: Some(flare_proto::access_gateway::PushStatistics {
                total_users: req.target_user_ids.len() as i32,
                online_users: success_count,
                offline_users: failure_count,
                success_count,
                failure_count,
            }),
        }))
    }

    async fn subscribe(
        &self,
        request: Request<flare_proto::access_gateway::SubscribeRequest>,
    ) -> Result<Response<flare_proto::access_gateway::SubscribeResponse>, Status> {
        let req = request.into_inner();
        info!("Subscribe request: user_id={}", req.user_id);

        // 实现订阅逻辑
        match self
            .subscription_service
            .subscribe(&req.user_id, &req.subscriptions)
            .await
        {
            Ok(granted_subscriptions) => {
                info!(
                    user_id = %req.user_id,
                    count = granted_subscriptions.len(),
                    "User subscribed to topics successfully"
                );

                Ok(Response::new(
                    flare_proto::access_gateway::SubscribeResponse {
                        granted: granted_subscriptions,
                        status: Some(flare_proto::RpcStatus {
                            code: flare_proto::common::ErrorCode::Ok as i32,
                            message: "Subscribed successfully".to_string(),
                            details: vec![],
                            context: None,
                        }),
                    },
                ))
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    user_id = %req.user_id,
                    "Failed to subscribe to topics"
                );

                Err(Status::internal(format!("Failed to subscribe: {}", e)))
            }
        }
    }

    async fn unsubscribe(
        &self,
        request: Request<flare_proto::access_gateway::UnsubscribeRequest>,
    ) -> Result<Response<flare_proto::access_gateway::UnsubscribeResponse>, Status> {
        let req = request.into_inner();
        info!("Unsubscribe request: user_id={}", req.user_id);

        // 实现取消订阅逻辑
        match self
            .subscription_service
            .unsubscribe(&req.user_id, &req.topics)
            .await
        {
            Ok(_) => {
                info!(
                    user_id = %req.user_id,
                    count = req.topics.len(),
                    "User unsubscribed from topics successfully"
                );

                Ok(Response::new(
                    flare_proto::access_gateway::UnsubscribeResponse {
                        status: Some(flare_proto::RpcStatus {
                            code: flare_proto::common::ErrorCode::Ok as i32,
                            message: "Unsubscribed successfully".to_string(),
                            details: vec![],
                            context: None,
                        }),
                    },
                ))
            }
            Err(e) => {
                tracing::error!(
                    error = %e,
                    user_id = %req.user_id,
                    "Failed to unsubscribe from topics"
                );

                Err(Status::internal(format!("Failed to unsubscribe: {}", e)))
            }
        }
    }

    async fn publish_signal(
        &self,
        request: Request<flare_proto::access_gateway::PublishSignalRequest>,
    ) -> Result<Response<flare_proto::access_gateway::PublishSignalResponse>, Status> {
        let req = request.into_inner();
        info!("PublishSignal request received");

        // 实现发布信令逻辑
        // 1. 从请求中提取信令信封
        let envelope = req
            .envelope
            .ok_or_else(|| Status::invalid_argument("envelope is required"))?;

        // 2. 获取订阅了指定主题的所有用户
        let subscribers = self.subscription_service.get_topic_subscribers(&envelope.topic).await
            .map_err(|e| {
                tracing::error!(error = %e, topic = %envelope.topic, "Failed to get topic subscribers");
                Status::internal(format!("Failed to get topic subscribers: {}", e))
            })?;

        // 3. 如果指定了目标用户，则只向这些用户推送
        let target_users = if !envelope.targets.is_empty() {
            // 过滤出既在目标列表中又订阅了主题的用户
            subscribers
                .into_iter()
                .filter(|user_id| envelope.targets.contains(user_id))
                .collect()
        } else {
            // 否则向所有订阅者推送
            subscribers
        };

        // 4. 构建信令消息载荷
        let signal_payload = flare_proto::common::ServerPacket {
            payload: Some(flare_proto::common::server_packet::Payload::CustomPushData(
                flare_proto::common::CustomPushData {
                    r#type: "signal".to_string(),
                    payload: envelope.payload.clone(),
                    metadata: envelope.metadata.clone(),
                },
            )),
        };

        // 5. 向每个目标用户推送信令消息
        let mut success_count = 0;
        let mut failure_count = 0;

        for user_id in &target_users {
            match self
                .connection_handler
                .push_packet_to_user(user_id, &signal_payload)
                .await
            {
                Ok(_) => {
                    success_count += 1;
                    tracing::debug!(
                        user_id = %user_id,
                        topic = %envelope.topic,
                        "Signal published successfully"
                    );
                }
                Err(err) => {
                    failure_count += 1;
                    tracing::warn!(
                        error = %err,
                        user_id = %user_id,
                        topic = %envelope.topic,
                        "Failed to publish signal to user"
                    );
                }
            }
        }

        info!(
            topic = %envelope.topic,
            from = %envelope.from,
            target_count = target_users.len(),
            success_count = success_count,
            failure_count = failure_count,
            "Signal published to subscribers"
        );

        Ok(Response::new(
            flare_proto::access_gateway::PublishSignalResponse {
                status: Some(flare_proto::RpcStatus {
                    code: flare_proto::common::ErrorCode::Ok as i32,
                    message: format!(
                        "Signal published: {} success, {} failures",
                        success_count, failure_count
                    ),
                    details: vec![],
                    context: None,
                }),
            },
        ))
    }
}
