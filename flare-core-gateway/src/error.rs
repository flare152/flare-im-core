use flare_core::common::error::{ErrorCode as CoreErrorCode, FlareError as CoreFlareError};
use flare_proto::common::RpcStatus;
use flare_server_core::error::{
    ErrorCode as ServerErrorCode, FlareError as ServerFlareError, from_rpc_status, to_rpc_status,
};

/// 将服务端 `FlareError` 转换为 gateway/client 所使用的 `flare-core` 错误
pub fn server_error_to_core(error: ServerFlareError) -> CoreFlareError {
    match error {
        ServerFlareError::Localized {
            code,
            reason,
            details,
            params,
            timestamp,
        } => {
            let mapped_code = map_server_code_to_core(code);
            CoreFlareError::Localized {
                code: mapped_code,
                reason,
                details,
                params,
                timestamp,
            }
        }
        ServerFlareError::System(message) => CoreFlareError::System(message),
        ServerFlareError::Io(message) => CoreFlareError::Io(message),
    }
}

/// 将 `flare-core` 错误转换为服务端错误类型，方便重用 server-core 的工具函数
pub fn core_error_to_server(error: &CoreFlareError) -> ServerFlareError {
    match error.clone() {
        CoreFlareError::Localized {
            code,
            reason,
            details,
            params,
            timestamp,
        } => {
            let mapped_code = map_core_code_to_server(code);
            ServerFlareError::Localized {
                code: mapped_code,
                reason,
                details,
                params,
                timestamp,
            }
        }
        CoreFlareError::System(message) => ServerFlareError::System(message),
        CoreFlareError::Io(message) => ServerFlareError::Io(message),
    }
}

/// 将 RPC Status 转换成 `flare-core` 错误
pub fn rpc_status_to_core(status: &RpcStatus) -> CoreFlareError {
    let server_err = from_rpc_status(status);
    server_error_to_core(server_err)
}

/// 将 `flare-core` 错误封装为 RPC Status，方便回传给下游
pub fn core_error_to_rpc_status(error: &CoreFlareError) -> RpcStatus {
    let server_err = core_error_to_server(error);
    to_rpc_status(&server_err)
}

fn map_server_code_to_core(code: ServerErrorCode) -> CoreErrorCode {
    CoreErrorCode::from_u32(code.as_u32()).unwrap_or(CoreErrorCode::InternalError)
}

fn map_core_code_to_server(code: CoreErrorCode) -> ServerErrorCode {
    ServerErrorCode::from_u32(code.as_u32()).unwrap_or(ServerErrorCode::InternalError)
}
