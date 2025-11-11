use std::env;

#[derive(Debug, Clone)]
pub struct RouteConfig {
    pub default_services: Vec<(String, String)>,
}

impl RouteConfig {
    pub fn from_env() -> Self {
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

        Self {
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
        }
    }
}
