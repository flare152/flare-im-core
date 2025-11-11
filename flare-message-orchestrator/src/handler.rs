use std::sync::Arc;

use anyhow::{Result, anyhow};
use flare_im_core::error::{
    ErrorBuilder, ErrorCode, map_error_code_to_proto, ok_status, to_rpc_status,
};
use flare_im_core::hooks::adapters::DefaultHookFactory;
use flare_im_core::hooks::{HookConfigLoader, HookDispatcher, HookRegistry};
use flare_proto::storage::storage_service_client::StorageServiceClient;
use flare_proto::storage::*;
use rdkafka::config::ClientConfig;
use rdkafka::producer::FutureProducer;
use tonic::{Request, Response, Status};
use tracing::{error, info};

use crate::application::commands::store_message::{
    StoreMessageCommand, StoreMessageCommandHandler,
};
use crate::domain::repositories::WalRepository;
use crate::infrastructure::config::MessageOrchestratorConfig;
use crate::infrastructure::messaging::kafka_publisher::KafkaMessagePublisher;
use crate::infrastructure::persistence::noop_wal::NoopWalRepository;
use crate::infrastructure::persistence::redis_wal::RedisWalRepository;

pub struct MessageOrchestratorHandler {
    config: Arc<MessageOrchestratorConfig>,
    store_handler: Arc<StoreMessageCommandHandler>,
    reader_endpoint: Option<String>,
}

impl MessageOrchestratorHandler {
    pub async fn new(config: Arc<MessageOrchestratorConfig>) -> Result<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_bootstrap)
            .set("message.timeout.ms", &config.kafka_timeout_ms.to_string())
            .create()?;
        let producer = Arc::new(producer);

        let publisher = Arc::new(KafkaMessagePublisher::new(producer, config.clone()));
        let wal_repository = Self::build_wal_repository(&config)?;

        info!(
            bootstrap = %config.kafka_bootstrap,
            topic = %config.kafka_topic,
            "MessageOrchestrator connected to Kafka"
        );

        let mut hook_loader = HookConfigLoader::new();
        if let Some(path) = &config.hook_config {
            hook_loader = hook_loader.add_candidate(path.clone());
        }
        if let Some(dir) = &config.hook_config_dir {
            hook_loader = hook_loader.add_candidate(dir.clone());
        }
        let hook_config = hook_loader.load().map_err(|err| anyhow!(err.to_string()))?;
        let registry = HookRegistry::builder().build();
        let hook_factory = DefaultHookFactory::new().map_err(|err| anyhow!(err.to_string()))?;
        hook_config
            .install(Arc::clone(&registry), &hook_factory)
            .await
            .map_err(|err| anyhow!(err.to_string()))?;
        let hooks = Arc::new(HookDispatcher::new(registry));

        let store_handler = Arc::new(StoreMessageCommandHandler::new(
            publisher,
            wal_repository,
            config.defaults(),
            Arc::clone(&hooks),
        ));

        Ok(Self {
            reader_endpoint: config.reader_endpoint.clone(),
            config,
            store_handler,
        })
    }

    fn build_wal_repository(
        config: &Arc<MessageOrchestratorConfig>,
    ) -> Result<Arc<dyn WalRepository + Send + Sync>> {
        if let Some(url) = &config.redis_url {
            let client = Arc::new(redis::Client::open(url.as_str())?);
            let repo: Arc<dyn WalRepository + Send + Sync> =
                Arc::new(RedisWalRepository::new(client, config.clone()));
            Ok(repo)
        } else {
            Ok(NoopWalRepository::shared())
        }
    }

    pub async fn handle_store_message(
        &self,
        request: Request<StoreMessageRequest>,
    ) -> Result<Response<StoreMessageResponse>, Status> {
        match self
            .store_handler
            .handle(StoreMessageCommand {
                request: request.into_inner(),
            })
            .await
        {
            Ok(message_id) => Ok(Response::new(StoreMessageResponse {
                success: true,
                message_id,
                error_message: String::new(),
                status: Some(ok_status()),
            })),
            Err(err) => {
                error!(error = ?err, "Failed to orchestrate message");
                Ok(Response::new(StoreMessageResponse {
                    success: false,
                    message_id: String::new(),
                    error_message: err.to_string(),
                    status: Some(to_rpc_status(&err)),
                }))
            }
        }
    }

    pub async fn handle_batch_store_message(
        &self,
        request: Request<BatchStoreMessageRequest>,
    ) -> Result<Response<BatchStoreMessageResponse>, Status> {
        let mut success_count = 0;
        let mut fail_count = 0;
        let mut message_ids = Vec::new();
        let mut failures = Vec::new();

        for message in request.into_inner().messages {
            let message_id_hint = message
                .message
                .as_ref()
                .map(|msg| msg.id.clone())
                .unwrap_or_default();

            match self
                .store_handler
                .handle(StoreMessageCommand { request: message })
                .await
            {
                Ok(message_id) => {
                    success_count += 1;
                    message_ids.push(message_id);
                }
                Err(err) => {
                    fail_count += 1;
                    error!(error = ?err, "Failed to enqueue message in batch");
                    let error_code = err.code().unwrap_or(ErrorCode::OperationFailed);
                    let proto_code = map_error_code_to_proto(error_code) as i32;
                    failures.push(FailedMessage {
                        message_id: message_id_hint.clone(),
                        code: proto_code,
                        error_message: err.to_string(),
                    });
                }
            }
        }

        let status = if fail_count == 0 {
            ok_status()
        } else {
            to_rpc_status(
                &ErrorBuilder::new(
                    ErrorCode::OperationFailed,
                    format!("{fail_count} messages failed to enqueue"),
                )
                .build_error(),
            )
        };

        Ok(Response::new(BatchStoreMessageResponse {
            success_count,
            fail_count,
            message_ids,
            failures,
            status: Some(status),
        }))
    }

    pub async fn handle_query_messages(
        &self,
        request: Request<QueryMessagesRequest>,
    ) -> Result<Response<QueryMessagesResponse>, Status> {
        let endpoint = self
            .reader_endpoint
            .as_ref()
            .ok_or_else(|| Status::unavailable("storage reader endpoint not configured"))?
            .clone();

        let mut client = StorageServiceClient::connect(endpoint.clone())
            .await
            .map_err(|err| {
                error!(error = ?err, endpoint = %endpoint, "Failed to connect storage reader");
                Status::unavailable("storage reader unavailable")
            })?;

        let response = client
            .query_messages(request)
            .await
            .map_err(|err| {
                error!(error = ?err, "Failed to query messages from reader");
                Status::internal("query storage reader failed")
            })?
            .into_inner();

        Ok(Response::new(response))
    }

    pub async fn handle_delete_message(
        &self,
        _request: Request<DeleteMessageRequest>,
    ) -> Result<Response<DeleteMessageResponse>, Status> {
        Ok(Response::new(DeleteMessageResponse {
            success: false,
            deleted_count: 0,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_recall_message(
        &self,
        _request: Request<RecallMessageRequest>,
    ) -> Result<Response<RecallMessageResponse>, Status> {
        Ok(Response::new(RecallMessageResponse {
            success: false,
            error_message: "operation not implemented".to_string(),
            recalled_at: None,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_delete_message_for_user(
        &self,
        _request: Request<DeleteMessageForUserRequest>,
    ) -> Result<Response<DeleteMessageForUserResponse>, Status> {
        Ok(Response::new(DeleteMessageForUserResponse {
            success: false,
            error_message: "operation not implemented".to_string(),
            visibility_status: VisibilityStatus::Hidden as i32,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_clear_session(
        &self,
        _request: Request<ClearSessionRequest>,
    ) -> Result<Response<ClearSessionResponse>, Status> {
        Ok(Response::new(ClearSessionResponse {
            success: false,
            error_message: "operation not implemented".to_string(),
            cleared_count: 0,
            cleared_at: None,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_mark_message_read(
        &self,
        _request: Request<MarkMessageReadRequest>,
    ) -> Result<Response<MarkMessageReadResponse>, Status> {
        Ok(Response::new(MarkMessageReadResponse {
            success: false,
            error_message: "operation not implemented".to_string(),
            read_at: None,
            burned_at: None,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_create_session(
        &self,
        _request: Request<CreateSessionRequest>,
    ) -> Result<Response<CreateSessionResponse>, Status> {
        Ok(Response::new(CreateSessionResponse {
            success: false,
            session_id: String::new(),
            session: None,
            error_message: "operation not implemented".to_string(),
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_get_session(
        &self,
        _request: Request<GetSessionRequest>,
    ) -> Result<Response<GetSessionResponse>, Status> {
        Ok(Response::new(GetSessionResponse {
            success: false,
            session: None,
            error_message: "Not implemented yet".to_string(),
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_update_session(
        &self,
        _request: Request<UpdateSessionRequest>,
    ) -> Result<Response<UpdateSessionResponse>, Status> {
        Ok(Response::new(UpdateSessionResponse {
            success: false,
            error_message: "Not implemented yet".to_string(),
            session: None,
            status: Some(operation_not_supported_status()),
        }))
    }

    pub async fn handle_query_user_sessions(
        &self,
        _request: Request<QueryUserSessionsRequest>,
    ) -> Result<Response<QueryUserSessionsResponse>, Status> {
        Ok(Response::new(QueryUserSessionsResponse {
            sessions: vec![],
            next_cursor: String::new(),
            has_more: false,
            pagination: None,
            status: Some(operation_not_supported_status()),
        }))
    }
}

fn operation_not_supported_status() -> flare_proto::common::RpcStatus {
    to_rpc_status(
        &ErrorBuilder::new(
            ErrorCode::OperationNotSupported,
            "operation not implemented".to_string(),
        )
        .build_error(),
    )
}
