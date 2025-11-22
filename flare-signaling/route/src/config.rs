use anyhow::Result;
use flare_im_core::config::FlareAppConfig;
use std::env;

#[derive(Debug, Clone)]
pub struct RouteConfig {
    pub default_services: Vec<(String, String)>,
}

impl RouteConfig {
    /// 从应用配置加载（新方式，推荐）
    pub fn from_app_config(_app: &FlareAppConfig) -> Result<Self> {
        // 路由服务的默认服务端点通过环境变量配置（支持动态发现）
        // 使用新的 SVID 格式（svid.im, svid.customer, svid.ai.bot）
        let default: Vec<(String, String)> = vec![
            (
                "svid.im".to_string(),
                env::var("BUSINESS_SERVICE_IM_ENDPOINT")
                    .or_else(|_| env::var("MESSAGE_ORCHESTRATOR_SERVICE"))
                    .unwrap_or("message-orchestrator".to_string()),
            ),
            (
                "svid.customer".to_string(),
                env::var("BUSINESS_SERVICE_CS_ENDPOINT").unwrap_or_default(),
            ),
            (
                "svid.ai.bot".to_string(),
                env::var("BUSINESS_SERVICE_AI_ENDPOINT").unwrap_or_default(),
            ),
        ]
        .into_iter()
        .filter(|(_, endpoint)| !endpoint.is_empty())
        .collect();

        Ok(Self {
            default_services: if default.is_empty() {
                // 默认配置：至少配置 SVID_IM → message-orchestrator
                vec![
                    ("svid.im".to_string(), "message-orchestrator".to_string()),
                    // 其他业务系统可以通过服务发现或注册接口动态添加
                ]
            } else {
                default
            },
        })
    }
}
