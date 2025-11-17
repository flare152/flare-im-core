//! # 认证拦截器
//!
//! 实现Tower Service，将认证中间件集成到gRPC服务中。

use std::task::{Context, Poll};

use tower::Service;
use tonic::{Request, Status};

use crate::interceptor::GatewayInterceptor;

/// 认证拦截器Service
pub struct AuthInterceptorService<S> {
    inner: S,
    interceptor: GatewayInterceptor,
}

impl<S> AuthInterceptorService<S> {
    pub fn new(inner: S, interceptor: GatewayInterceptor) -> Self {
        Self { inner, interceptor }
    }
}

impl<S, ReqBody> Service<Request<ReqBody>> for AuthInterceptorService<S>
where
    S: Service<Request<ReqBody>, Response = tonic::Response<()>, Error = Status> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = futures::future::BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let mut inner = self.inner.clone();
        let interceptor = self.interceptor.clone();
        
        Box::pin(async move {
            // 提取metadata（在移动request之前）
            // 需要克隆metadata，因为async move块中不能持有引用跨越await点
            let metadata = req.metadata().clone();
            
            // 1. 认证：提取和验证Token
            let claims = match interceptor.auth_middleware.authenticate(&metadata) {
                Ok(claims) => claims,
                Err(e) => {
                    return Err(Status::unauthenticated(format!("Authentication failed: {}", e)));
                }
            };
            
            // 2. 租户上下文提取
            let tenant_context = crate::middleware::TenantMiddleware::extract_from_claims(&claims);
            
            // 3. 限流检查（提取client_ip）
            let client_ip = GatewayInterceptor::extract_client_ip(&metadata);
            if let Err(e) = interceptor.rate_limit_middleware.check_rate_limit(&claims, client_ip.as_deref()).await {
                return Err(Status::resource_exhausted(format!("Rate limit exceeded: {}", e)));
            }
            
            // 4. 将Claims和租户上下文注入到请求扩展中
            let mut req = req;
            req.extensions_mut().insert(claims);
            req.extensions_mut().insert(tenant_context);
            
            // 调用内部服务
            inner.call(req).await
        })
    }
}

// GatewayInterceptor的Clone实现移到mod.rs中

