use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::HookContext;

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
    pub session_types: MatchRule,
    #[serde(default)]
    pub message_types: MatchRule,
}

impl HookSelector {
    pub fn matches(&self, ctx: &HookContext) -> bool {
        self.tenants.matches(Some(ctx.tenant_id.as_str()))
            && self.session_types.matches(ctx.session_type.as_deref())
            && self.message_types.matches(ctx.message_type.as_deref())
    }
}
