//! 服务启动模块
//!
//! 统一管理服务启动、端口配置和启动信息展示

use crate::service::service_manager::PortConfig;
use crate::service::wire::ApplicationContext;
use anyhow::Result;
use std::net::SocketAddr;
use tracing::{error, info, warn};

/// 启动信息展示器
pub struct StartupInfo {
    /// 网关 ID
    pub gateway_id: String,
    /// 地区
    pub region: Option<String>,
    /// 端口配置
    pub port_config: PortConfig,
    /// 服务器地址
    pub address: String,
    /// gRPC 服务列表
    pub grpc_services: Vec<GrpcServiceInfo>,
}

/// gRPC 服务信息
#[derive(Debug, Clone)]
pub struct GrpcServiceInfo {
    /// 服务名称
    pub name: String,
    /// 服务描述
    pub description: String,
}

impl StartupInfo {
    /// 创建启动信息
    pub fn new(
        gateway_id: String,
        region: Option<String>,
        port_config: PortConfig,
        address: String,
    ) -> Self {
        Self {
            gateway_id,
            region,
            port_config,
            address,
            grpc_services: vec![
                GrpcServiceInfo {
                    name: "SignalingService".to_string(),
                    description: "认证、会话管理".to_string(),
                },
                GrpcServiceInfo {
                    name: "AccessGateway".to_string(),
                    description: "业务系统推送消息".to_string(),
                },
            ],
        }
    }

    /// 打印启动信息
    pub fn print(&self) {
        info!("");
        info!("╔════════════════════════════════════════════════════════════════╗");
        info!("║          Flare Access Gateway 服务启动成功                    ║");
        info!("╚════════════════════════════════════════════════════════════════╝");
        info!("");
        
        // 网关信息
        info!("📋 网关信息:");
        info!("   Gateway ID: {}", self.gateway_id);
        if let Some(ref region) = self.region {
            info!("   Region:     {}", region);
        }
        info!("");
        
        // gRPC 服务信息
        info!("🔌 gRPC 服务 (服务间调用，已注册到服务注册中心):");
        let grpc_addr = format!("{}:{}", self.address, self.port_config.grpc_port);
        info!("   gRPC 地址:  {}", grpc_addr);
        info!("");
        info!("   服务列表:");
        for service in &self.grpc_services {
            info!("     • {} - {}", service.name, service.description);
        }
        info!("");
        
        // 长连接服务信息
        info!("🌐 长连接服务 (客户端连接):");
        let ws_addr = format!("{}:{}", self.address, self.port_config.ws_port);
        let quic_addr = format!("{}:{}", self.address, self.port_config.quic_port);
        info!("   WebSocket:  {} (ws://{} 或 wss://{})", 
            ws_addr, ws_addr, ws_addr);
        info!("   QUIC:       {} (quic://{})", quic_addr, quic_addr);
        info!("");
        
        // 端口映射说明
        info!("📝 端口说明:");
        info!("   • gRPC 端口 ({}) 用于服务间调用，已注册到服务注册中心", 
            self.port_config.grpc_port);
        info!("   • WebSocket 端口 ({}) 用于客户端 WebSocket 连接", 
            self.port_config.ws_port);
        info!("   • QUIC 端口 ({}) 用于客户端 QUIC 连接", 
            self.port_config.quic_port);
        info!("");
        
        // 连接示例
        info!("💡 连接示例:");
        info!("   客户端连接 WebSocket:");
        info!("     ws://{}/ws", ws_addr);
        info!("   客户端连接 QUIC:");
        info!("     quic://{}", quic_addr);
        info!("   业务系统调用 gRPC:");
        info!("     grpc://{}", grpc_addr);
        info!("");
        
        info!("✅ 所有服务已就绪，等待客户端连接...");
        info!("");
    }

    /// 获取 gRPC 地址
    pub fn grpc_addr(&self) -> Result<SocketAddr> {
        format!("{}:{}", self.address, self.port_config.grpc_port)
            .parse()
            .map_err(|err| anyhow::anyhow!("Invalid gRPC address: {}", err))
    }

    /// 获取 WebSocket 地址
    pub fn ws_addr(&self) -> String {
        format!("{}:{}", self.address, self.port_config.ws_port)
    }

    /// 获取 QUIC 地址
    pub fn quic_addr(&self) -> String {
        format!("{}:{}", self.address, self.port_config.quic_port)
    }
}

/// 启动服务
pub async fn start_services(
    context: ApplicationContext,
    port_config: PortConfig,
    address: String,
    gateway_id: String,
    region: Option<String>,
) -> Result<()> {
    use flare_server_core::runtime::ServiceRuntime;
    use tonic::transport::Server;
    
    // 创建启动信息
    let startup_info = StartupInfo::new(
        gateway_id.clone(),
        region.clone(),
        port_config.clone(),
        address.clone(),
    );

    // 打印启动信息
    startup_info.print();

    // 解析 gRPC 地址
    let grpc_addr: SocketAddr = format!("{}:{}", address, port_config.grpc_port)
        .parse()
        .map_err(|err| anyhow::anyhow!("Invalid gRPC address: {}", err))?;

    // 获取 gRPC 处理器
    let signaling_handler = context.grpc_services.signaling_handler.clone();
    let access_gateway_handler = context.grpc_services.access_gateway_handler.clone();
    
    // 长连接服务器已在 wire.rs 中启动，这里只需要确保它正常运行
    // 验证长连接服务器是否已启动
    {
        let server_guard = context.long_connection_server.lock().await;
        if server_guard.is_some() {
            info!("✅ 长连接服务器已在 wire.rs 中启动");
        } else {
            error!("❌ 长连接服务器未启动");
            return Err(anyhow::anyhow!("长连接服务器未启动"));
        }
    }

    // 获取长连接服务器（用于优雅停机）
    let long_connection_server = context.long_connection_server.clone();

    // 使用 ServiceRuntime 统一管理服务生命周期
    let runtime = ServiceRuntime::new("access-gateway", grpc_addr)
        // 添加 gRPC 服务任务
        .add_spawn_with_shutdown("grpc-server", move |shutdown_rx| async move {
            info!("正在启动 gRPC 服务器: {}", grpc_addr);
            
            let server_result = Server::builder()
                .add_service(
                    flare_proto::signaling::signaling_service_server::SignalingServiceServer::new(
                        (*signaling_handler).clone(),
                    ),
                )
                .add_service(
                    flare_proto::access_gateway::access_gateway_server::AccessGatewayServer::new(
                        (*access_gateway_handler).clone(),
                    ),
                )
                .serve_with_shutdown(grpc_addr, async move {
                    info!(
                        address = %grpc_addr,
                        port = %grpc_addr.port(),
                        "✅ Access Gateway gRPC service is listening"
                    );
                    
                    // 同时监听 Ctrl+C 和关闭通道
                    tokio::select! {
                        _ = tokio::signal::ctrl_c() => {
                            tracing::info!("shutdown signal received (Ctrl+C)");
                        }
                        _ = shutdown_rx => {
                            tracing::info!("shutdown signal received (service registration failed)");
                        }
                    }
                })
                .await;

            match server_result {
                Ok(_) => {
                    info!("gRPC 服务器已停止");
                    Ok(())
                }
                Err(e) => {
                    error!(error = %e, "gRPC 服务器启动失败");
                    Err(format!("gRPC server error: {}", e).into())
                }
            }
        });

    // 运行服务（带服务注册）
    let gateway_id_for_reg = gateway_id.clone();
    let region_for_reg = region.clone();
    let long_connection_server_for_cleanup = long_connection_server.clone();
    
    runtime.run_with_registration(move |addr| {
        let gateway_id_clone = gateway_id_for_reg.clone();
        let region_clone = region_for_reg.clone();
        
        Box::pin(async move {
            // 注册服务（使用常量）
            use flare_im_core::service_names::ACCESS_GATEWAY;
            match flare_im_core::discovery::register_service_only(ACCESS_GATEWAY, addr, Some(gateway_id_clone.clone())).await {
                Ok(Some(registry)) => {
                    info!(
                        "✅ Service registered: {} (instance_id={}, region={:?})",
                        ACCESS_GATEWAY,
                        gateway_id_clone,
                        region_clone
                    );
                    Ok(Some(registry))
                }
                Ok(None) => {
                    info!("Service discovery not configured, skipping registration");
                    Ok(None)
                }
                Err(e) => {
                    error!(
                        error = %e,
                        "❌ Service registration failed"
                    );
                    Err(format!("Service registration failed: {}", e).into())
                }
            }
        })
    }).await?;
    
    // ServiceRuntime 停止后，停止长连接服务器
    if let Some(mut server) = long_connection_server_for_cleanup.lock().await.take() {
        info!("正在停止长连接服务器...");
        if let Err(e) = server.stop().await {
            warn!(error = %e, "停止长连接服务器失败");
        } else {
            info!("长连接服务器已停止");
        }
    }
    
    Ok(())
}
