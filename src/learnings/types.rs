use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

// ── Scope ────────────────────────────────────────────────────────

/// Where and when a learning is injected.
///
/// - `Global`  — always injected into the system prompt
/// - `Skill`   — injected alongside a specific skill's instructions
/// - `Channel` — injected when a message arrives from a matching channel/chat
/// - `Hook`    — injected at a named hook point (e.g. `on_message_received`,
///               `before_tool_call`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LearningScope {
    /// Always active — injected into every system prompt.
    Global,

    /// Activate only when the named skill appears in the active skills list.
    ///
    /// Value is the skill name (e.g. `"feature-pr"`).
    Skill { skill: String },

    /// Activate when the inbound message originates from a matching channel.
    ///
    /// Value is a channel identifier string: `"slack:#channel-name"`,
    /// `"discord:channel-id"`, `"telegram:chat-id"`, or a bare name for
    /// provider-agnostic matching.
    Channel { channel: String },

    /// Activate at a named hook point.
    ///
    /// Value is the hook method name (e.g. `"before_tool_call"`,
    /// `"on_message_received"`).  The learning text is appended to the prompt
    /// immediately before the hook fires.
    Hook { hook: String },
}

impl fmt::Display for LearningScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LearningScope::Global => write!(f, "global"),
            LearningScope::Skill { skill } => write!(f, "skill:{skill}"),
            LearningScope::Channel { channel } => write!(f, "channel:{channel}"),
            LearningScope::Hook { hook } => write!(f, "hook:{hook}"),
        }
    }
}

// ── Learning ─────────────────────────────────────────────────────

/// A single learning — a soft behavioral rule applied at a specific scope.
///
/// Learnings live in `<workspace>/learnings/<name>/LEARNING.toml` (+ optional
/// `LEARNING.md` for long-form rule text).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learning {
    pub name: String,
    pub description: String,

    #[serde(default = "default_version")]
    pub version: String,

    #[serde(default)]
    pub author: Option<String>,

    #[serde(default)]
    pub tags: Vec<String>,

    /// One or more scopes — a learning can apply in multiple contexts.
    pub scopes: Vec<LearningScope>,

    /// The actual behavioral rules/instructions.
    #[serde(default)]
    pub rules: Vec<String>,

    /// Filesystem location of the LEARNING.toml (not serialized to TOML).
    #[serde(skip)]
    pub location: Option<PathBuf>,
}

fn default_version() -> String {
    "0.1.0".to_string()
}

// ── Manifest (LEARNING.toml on disk) ─────────────────────────────

/// Top-level LEARNING.toml structure.
#[derive(Debug, Deserialize)]
pub(crate) struct LearningManifest {
    pub learning: LearningMeta,
    #[serde(default)]
    pub rules: Vec<LearningRule>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LearningMeta {
    pub name: String,
    pub description: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Convenience single-scope shorthand.
    #[serde(default)]
    pub scope: Option<ScopeShorthand>,
    /// Multi-scope list (preferred for learnings with multiple contexts).
    #[serde(default)]
    pub scopes: Vec<ScopeShorthand>,
}

/// A rule entry inside `[[rules]]`.
#[derive(Debug, Deserialize)]
pub(crate) struct LearningRule {
    pub content: String,
}

/// Human-readable scope string parsed from TOML.
///
/// Accepted formats:
///   `"global"`, `"skill:feature-pr"`, `"channel:slack:#dev"`,
///   `"hook:before_tool_call"`
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub(crate) struct ScopeShorthand(pub String);

impl ScopeShorthand {
    pub fn parse(&self) -> Option<LearningScope> {
        parse_scope_shorthand(&self.0)
    }
}

pub(crate) fn parse_scope_shorthand(s: &str) -> Option<LearningScope> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("global") {
        return Some(LearningScope::Global);
    }
    if let Some(rest) = s.strip_prefix("skill:") {
        if !rest.is_empty() {
            return Some(LearningScope::Skill {
                skill: rest.to_string(),
            });
        }
    }
    if let Some(rest) = s.strip_prefix("channel:") {
        if !rest.is_empty() {
            return Some(LearningScope::Channel {
                channel: rest.to_string(),
            });
        }
    }
    if let Some(rest) = s.strip_prefix("hook:") {
        if !rest.is_empty() {
            return Some(LearningScope::Hook {
                hook: rest.to_string(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_global_scope() {
        assert_eq!(parse_scope_shorthand("global"), Some(LearningScope::Global));
        assert_eq!(parse_scope_shorthand("GLOBAL"), Some(LearningScope::Global));
    }

    #[test]
    fn parse_skill_scope() {
        assert_eq!(
            parse_scope_shorthand("skill:feature-pr"),
            Some(LearningScope::Skill {
                skill: "feature-pr".into()
            })
        );
    }

    #[test]
    fn parse_channel_scope() {
        assert_eq!(
            parse_scope_shorthand("channel:slack:#sct-internal-dev"),
            Some(LearningScope::Channel {
                channel: "slack:#sct-internal-dev".into()
            })
        );
    }

    #[test]
    fn parse_hook_scope() {
        assert_eq!(
            parse_scope_shorthand("hook:before_tool_call"),
            Some(LearningScope::Hook {
                hook: "before_tool_call".into()
            })
        );
    }

    #[test]
    fn parse_invalid_scope_returns_none() {
        assert_eq!(parse_scope_shorthand("unknown:foo"), None);
        assert_eq!(parse_scope_shorthand("skill:"), None);
        assert_eq!(parse_scope_shorthand(""), None);
    }

    #[test]
    fn display_scope() {
        assert_eq!(LearningScope::Global.to_string(), "global");
        assert_eq!(
            LearningScope::Skill {
                skill: "gh-issues".into()
            }
            .to_string(),
            "skill:gh-issues"
        );
        assert_eq!(
            LearningScope::Channel {
                channel: "slack:#dev".into()
            }
            .to_string(),
            "channel:slack:#dev"
        );
        assert_eq!(
            LearningScope::Hook {
                hook: "on_message_received".into()
            }
            .to_string(),
            "hook:on_message_received"
        );
    }
}
