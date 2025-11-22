use flare_proto::common::RpcStatus;
use flare_server_core::error::ok_status;

pub fn rpc_status_ok() -> RpcStatus {
    ok_status()
}
