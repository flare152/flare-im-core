//! 连接生命周期管理模块
//!
//! 处理连接建立和断开事件

use flare_core::common::error::Result as CoreResult;
use tracing::warn;
use tracing::instrument;

use super::connection::LongConnectionHandler;

impl LongConnectionHandler {
    /// 连接建立时的内部实现（协议适配层）
    #[instrument(skip(self), fields(connection_id))]
    pub(crate) async fn on_connect_impl(&self, connection_id: &str) -> CoreResult<()> {
        // 获取当前活跃连接数
        let active_count = self.server_handle
            .lock()
            .await
            .as_ref()
            .map(|h| h.connection_count())
            .unwrap_or(0);

        // 获取连接信息并处理
        if let Some((user_id, device_id)) = self.get_connection_info(connection_id).await {
            if let Err(err) = self
                .connection_handler
                .handle_connect(connection_id, &user_id, &device_id, active_count)
                .await
            {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to handle connection"
                );
            }
        } else {
            warn!(
                connection_id = %connection_id,
                "Connection established but connection info not found"
            );
        }

        Ok(())
    }

    /// 连接断开时的内部实现（协议适配层）
    #[instrument(skip(self), fields(connection_id))]
    pub(crate) async fn on_disconnect_impl(&self, connection_id: &str) -> CoreResult<()> {
        // 获取当前活跃连接数
        let active_count = self.server_handle
            .lock()
            .await
            .as_ref()
            .map(|h| h.connection_count())
            .unwrap_or(0);

        // 获取 user_id 并处理断开
        if let Some(user_id) = self.user_id_for_connection(connection_id).await {
            // 检查是否还有其他连接（在断开前，连接数 > 1 表示还有其他连接）
            let has_other_connections = if let Some(ref manager) = *self.manager_trait.lock().await {
                let count = manager.connection_count().await;
                count > 1 // 当前连接还未移除，所以 > 1 表示还有其他连接
            } else {
                false
            };

            // 委托给应用层服务处理
            if let Err(err) = self
                .connection_handler
                .handle_disconnect(connection_id, &user_id, active_count, has_other_connections)
                .await
            {
                warn!(
                    ?err,
                    user_id = %user_id,
                    connection_id = %connection_id,
                    "Failed to handle disconnection"
                );
            }
        }

        Ok(())
    }
}
