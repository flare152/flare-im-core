use flare_im_core::config::FlareAppConfig;
use std::env;

#[derive(Debug, Clone)]
pub struct PushWorkerConfig {
    pub kafka_bootstrap: String,
    pub consumer_group: String,
    pub task_topic: String,
    pub signaling_endpoint: Option<String>,
    pub offline_provider: Option<String>,
    pub hook_config: Option<String>,
    pub hook_config_dir: Option<String>,
}

impl PushWorkerConfig {
    pub fn from_app_config(app: &FlareAppConfig) -> Self {
        let service = app.push_worker_service();
        let kafka_name = service.kafka.as_deref().unwrap_or("push");
        let kafka_profile = app.kafka_profile(kafka_name);

        let kafka_bootstrap = env::var("PUSH_WORKER_KAFKA_BOOTSTRAP")
            .ok()
            .or_else(|| kafka_profile.map(|cfg| cfg.bootstrap_servers.clone()))
            .unwrap_or_else(|| "localhost:9092".to_string());

        let consumer_group =
            env::var("PUSH_WORKER_CONSUMER_GROUP").unwrap_or_else(|_| "push-worker".to_string());

        let task_topic = env::var("PUSH_WORKER_TASK_TOPIC")
            .ok()
            .or_else(|| service.task_topic.clone())
            .unwrap_or_else(|| "push-tasks".to_string());

        let signaling_endpoint = env::var("PUSH_WORKER_SIGNALING_ENDPOINT")
            .ok()
            .or_else(|| service.signaling_endpoint.clone());
        let offline_provider = env::var("PUSH_WORKER_OFFLINE_PROVIDER")
            .ok()
            .or_else(|| service.offline_provider.clone());

        let hook_config = env::var("PUSH_WORKER_HOOKS_CONFIG")
            .ok()
            .or_else(|| service.hook_config.clone());
        let hook_config_dir = env::var("PUSH_WORKER_HOOKS_CONFIG_DIR")
            .ok()
            .or_else(|| service.hook_config_dir.clone());

        Self {
            kafka_bootstrap,
            consumer_group,
            task_topic,
            signaling_endpoint,
            offline_provider,
            hook_config,
            hook_config_dir,
        }
    }
}
