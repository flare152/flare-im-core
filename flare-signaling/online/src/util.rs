use flare_proto::common::RpcStatus;
use flare_server_core::error::{ErrorBuilder, ErrorCode, ok_status, to_rpc_status};

pub fn rpc_status_ok() -> Option<RpcStatus> {
    Some(ok_status())
}

pub fn rpc_status_error(code: ErrorCode, message: &str) -> Option<RpcStatus> {
    let error = ErrorBuilder::new(code, message).build_error();
    Some(to_rpc_status(&error))
}
