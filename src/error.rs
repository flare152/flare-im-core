//! Flare IM Core 错误工具模块
//!
//! - 统一对外暴露 `flare-server-core` 定义的错误类型
//! - 为基础设施层提供便捷的错误转换工具

pub use flare_server_core::error::{
    ErrorBuilder, ErrorCategory, ErrorCode, FlareError, FlareServerError, GrpcError, GrpcErrorExt,
    GrpcResult, InfraResult, InfraResultExt, LocalizedError, Result, from_rpc_status,
    localized_to_rpc_status, map_error_code_to_proto, map_infra_error, map_proto_code_to_error,
    ok_status, to_localized, to_rpc_status,
};

/// 便捷宏：将基础设施错误映射为指定业务错误并提前返回
#[macro_export]
macro_rules! bail_infra {
    ($err:expr, $code:expr, $msg:expr) => {
        return Err($crate::error::map_infra_error($err, $code, $msg))
    };
}

/// 便捷宏：从返回 `InfraResult` 的表达式中直接转换为业务层 `Result`
#[macro_export]
macro_rules! try_infra {
    ($expr:expr, $code:expr, $msg:expr) => {
        $expr.into_flare($code, $msg)?
    };
}
