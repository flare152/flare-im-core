use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use flare_server_core::context::Context;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum MatchRule {
    Any,
    Exact { values: HashSet<String> },
}

impl Default for MatchRule {
    fn default() -> Self {
        MatchRule::Any
    }
}

impl MatchRule {
    pub fn any() -> Self {
        MatchRule::Any
    }

    pub fn of<I, T>(values: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        MatchRule::Exact {
            values: values.into_iter().map(Into::into).collect(),
        }
    }

    pub fn matches(&self, value: Option<&str>) -> bool {
        match self {
            MatchRule::Any => true,
            MatchRule::Exact { values } => value.map(|val| values.contains(val)).unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookSelector {
    #[serde(default)]
    pub tenants: MatchRule,
    #[serde(default)]
    pub conversation_types: MatchRule,
    #[serde(default)]
    pub message_types: MatchRule,
}

impl HookSelector {
    pub fn matches(&self, ctx: &Context) -> bool {
        use crate::hooks::hook_context_data::get_hook_context_data;
        
        let tenant_id = ctx.tenant_id().map(|s| s.to_string()).unwrap_or_default();
        let hook_data = get_hook_context_data(ctx);
        
        self.tenants.matches(Some(tenant_id.as_str()))
            && self.conversation_types.matches(
                hook_data.and_then(|d| d.conversation_type.as_deref())
            )
            && self.message_types.matches(
                hook_data.and_then(|d| d.message_type.as_deref())
            )
    }
}
