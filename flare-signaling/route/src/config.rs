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
        let default: Vec<(String, String)> = vec![
            (
                "IM".to_string(),
                env::var("BUSINESS_SERVICE_IM_ENDPOINT").unwrap_or_default(),
            ),
            (
                "CUSTOMER_SERVICE".to_string(),
                env::var("BUSINESS_SERVICE_CS_ENDPOINT").unwrap_or_default(),
            ),
            (
                "AI_BOT".to_string(),
                env::var("BUSINESS_SERVICE_AI_ENDPOINT").unwrap_or_default(),
            ),
        ]
        .into_iter()
        .filter(|(_, endpoint)| !endpoint.is_empty())
        .collect();

        Ok(Self {
            default_services: if default.is_empty() {
                vec![
                    ("IM".to_string(), "http://localhost:50091".to_string()),
                    (
                        "CUSTOMER_SERVICE".to_string(),
                        "http://localhost:50092".to_string(),
                    ),
                    ("AI_BOT".to_string(), "http://localhost:50093".to_string()),
                ]
            } else {
                default
            },
        })
    }
}
