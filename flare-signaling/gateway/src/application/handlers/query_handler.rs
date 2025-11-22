//! # Gateway查询处理器（编排层）
//!
//! 负责处理查询，调用领域服务

// 连接查询
mod connection_queries {
    use std::sync::Arc;

    use flare_proto::access_gateway::{
        ConnectionInfo as ProtoConnectionInfo, QueryUserConnectionsRequest,
        QueryUserConnectionsResponse, UserConnectionInfo,
    };
    use tonic::Status;
    use anyhow::Result;
    use flare_proto::RpcStatus;
    use flare_proto::common::ErrorCode;
    
    // Helper function to create OK status
    fn ok_status() -> Status {
        Status::ok("")
    }
    
    // Helper function to create OK RpcStatus
    fn ok_rpc_status() -> RpcStatus {
        RpcStatus {
            code: ErrorCode::Ok as i32,
            message: "".to_string(),
            details: vec![],
            context: None,
        }
    }
    use prost_types::Timestamp;

    use crate::domain::repository::ConnectionQuery;

    /// 查询用户连接状态查询
    pub struct QueryUserConnectionsQuery {
        pub request: QueryUserConnectionsRequest,
    }

    /// 连接查询服务
    pub struct ConnectionQueryService {
        connection_query: Arc<dyn ConnectionQuery>,
    }

    impl ConnectionQueryService {
        pub fn new(connection_query: Arc<dyn ConnectionQuery>) -> Self {
            Self { connection_query }
        }

        /// 处理查询用户连接状态请求
        pub async fn handle_query_user_connections(
            &self,
            query: QueryUserConnectionsQuery,
        ) -> Result<QueryUserConnectionsResponse> {
            let request = query.request;
            let user_ids = request.user_ids;

            let mut users = Vec::new();

            for user_id in user_ids {
                // 查询用户连接信息
                let connections = self
                    .connection_query
                    .query_user_connections(&user_id)
                    .await?;

                let online = !connections.is_empty();

                // 转换连接信息
                let proto_connections: Vec<ProtoConnectionInfo> = connections
                    .into_iter()
                    .map(|conn| ProtoConnectionInfo {
                        connection_id: conn.connection_id,
                        protocol: conn.protocol,
                        device_id: conn.device_id,
                        platform: conn.platform,
                        connected_at: conn.connected_at.map(|ts| Timestamp {
                            seconds: ts.timestamp(),
                            nanos: ts.timestamp_subsec_nanos() as i32,
                        }),
                        last_active_at: conn.last_active_at.map(|ts| Timestamp {
                            seconds: ts.timestamp(),
                            nanos: ts.timestamp_subsec_nanos() as i32,
                        }),
                    })
                    .collect();

                // 计算最后活跃时间
                let last_active_at = proto_connections
                    .iter()
                    .filter_map(|conn| conn.last_active_at.as_ref())
                    .max_by_key(|ts| (ts.seconds, ts.nanos))
                    .cloned();

                users.push(UserConnectionInfo {
                    user_id: user_id.clone(),
                    online,
                    connections: proto_connections,
                    last_active_at,
                });
            }

            Ok(QueryUserConnectionsResponse {
                users,
                status: Some(ok_rpc_status()),
            })
        }
    }
}

// 会话查询
mod session_queries {
    use std::sync::Arc;

    use flare_proto::signaling::{GetOnlineStatusRequest, GetOnlineStatusResponse};
    use flare_server_core::error::{Result, ok_status};

    use crate::domain::repository::SignalingGateway;

    pub struct GetOnlineStatusQuery {
        pub request: GetOnlineStatusRequest,
    }

    pub struct SessionQueryService {
        signaling: Arc<dyn SignalingGateway>,
    }

    impl SessionQueryService {
        pub fn new(signaling: Arc<dyn SignalingGateway>) -> Self {
            Self { signaling }
        }

        pub async fn handle_get_online_status(
            &self,
            query: GetOnlineStatusQuery,
        ) -> Result<GetOnlineStatusResponse> {
            let mut response = self.signaling.get_online_status(query.request).await?;
            if response.status.is_none() {
                response.status = Some(ok_status());
            }
            Ok(response)
        }
    }
}

// 导出
pub use connection_queries::{ConnectionQueryService, QueryUserConnectionsQuery};
pub use session_queries::{GetOnlineStatusQuery, SessionQueryService};

