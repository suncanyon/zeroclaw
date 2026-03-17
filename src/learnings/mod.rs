//! # Learnings
//!
//! Learnings are soft behavioral rules that extend an agent's behavior without
//! modifying its core identity.  Unlike `SOUL.md` / `AGENTS.md` (identity) or
//! skills (capabilities), learnings describe *how* an agent should act in
//! specific contexts.
//!
//! ## Scope model
//!
//! Each learning declares one or more scopes that control *when* and *where*
//! its rules are injected into the agent's context:
//!
//! | Scope             | Injected when…                                         |
//! |-------------------|---------------------------------------------------------|
//! | `global`          | Always — every system prompt                           |
//! | `skill:<name>`    | The named skill is active for the current request      |
//! | `channel:<id>`    | The inbound message originates from a matching channel |
//! | `hook:<hook>`     | A specific lifecycle hook fires                        |
//!
//! ## On-disk format
//!
//! ```
//! <workspace>/learnings/<name>/
//!   LEARNING.toml   ← required: metadata + rules
//!   LEARNING.md     ← optional: long-form prose injected as an additional rule
//! ```
//!
//! Minimal `LEARNING.toml`:
//!
//! ```toml
//! [learning]
//! name        = "draft-pr-first"
//! description = "Always draft PRs before promoting to review-ready"
//! scope       = "skill:feature-pr"
//!
//! [[rules]]
//! content = "Create PRs as drafts first. Do not promote to non-draft without explicit human approval."
//! ```
//!
//! Multi-scope example:
//!
//! ```toml
//! [learning]
//! name        = "sct-dev-channel-context"
//! description = "Contextual assumptions for #sct-internal-dev"
//! scopes      = ["channel:slack:#sct-internal-dev"]
//!
//! [[rules]]
//! content = "When the request is ambiguous, assume it relates to the buysidehub-api repository."
//!
//! [[rules]]
//! content = "Always link PRs and commits in replies — do not just say 'I updated file X'."
//! ```

mod store;
mod types;

pub use store::LearningsStore;
pub use types::{Learning, LearningScope};
use types::{LearningManifest, ScopeShorthand};

use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::warn;

// ── Directory helpers ────────────────────────────────────────────

pub fn learnings_dir(workspace_dir: &Path) -> PathBuf {
    workspace_dir.join("learnings")
}

// ── Loading ──────────────────────────────────────────────────────

/// Load all learnings from `<workspace>/learnings/`.
pub fn load_learnings(workspace_dir: &Path) -> Vec<Learning> {
    let dir = learnings_dir(workspace_dir);
    load_learnings_from_directory(&dir)
}

pub(crate) fn load_learnings_from_directory(learnings_dir: &Path) -> Vec<Learning> {
    if !learnings_dir.exists() {
        return Vec::new();
    }

    let mut learnings = Vec::new();

    let Ok(entries) = std::fs::read_dir(learnings_dir) else {
        return learnings;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let toml_path = path.join("LEARNING.toml");
        if !toml_path.exists() {
            continue;
        }

        match load_learning(&path) {
            Ok(learning) => learnings.push(learning),
            Err(e) => {
                warn!("Failed to load learning from {}: {e}", path.display());
            }
        }
    }

    learnings.sort_by(|a, b| a.name.cmp(&b.name));
    learnings
}

fn load_learning(dir: &Path) -> Result<Learning> {
    let toml_path = dir.join("LEARNING.toml");
    let toml_content = std::fs::read_to_string(&toml_path)?;
    let manifest: LearningManifest = toml::from_str(&toml_content)?;

    // Collect all scopes: `scope` (singular) + `scopes` (list)
    let mut raw_scopes: Vec<ScopeShorthand> = manifest.learning.scopes.clone();
    if let Some(ref singular) = manifest.learning.scope {
        raw_scopes.push(singular.clone());
    }

    let scopes: Vec<LearningScope> = raw_scopes
        .iter()
        .filter_map(|s| {
            let parsed = s.parse();
            if parsed.is_none() {
                warn!(
                    "Learning '{}': unrecognised scope '{}' — skipping",
                    manifest.learning.name, s.0
                );
            }
            parsed
        })
        .collect();

    if scopes.is_empty() {
        anyhow::bail!(
            "Learning '{}' has no valid scopes — add `scope = \"global\"` or another scope",
            manifest.learning.name
        );
    }

    // Base rules from [[rules]] entries
    let mut rules: Vec<String> = manifest.rules.iter().map(|r| r.content.clone()).collect();

    // Optional LEARNING.md appended as a final rule block
    let md_path = dir.join("LEARNING.md");
    if md_path.exists() {
        if let Ok(md) = std::fs::read_to_string(&md_path) {
            let trimmed = md.trim().to_string();
            if !trimmed.is_empty() {
                rules.push(trimmed);
            }
        }
    }

    Ok(Learning {
        name: manifest.learning.name,
        description: manifest.learning.description,
        version: manifest.learning.version,
        author: manifest.learning.author,
        tags: manifest.learning.tags,
        scopes,
        rules,
        location: Some(toml_path),
    })
}

// ── Filtering helpers ────────────────────────────────────────────

/// Return learnings that match `LearningScope::Global`.
pub fn global_learnings(learnings: &[Learning]) -> Vec<&Learning> {
    learnings
        .iter()
        .filter(|l| l.scopes.contains(&LearningScope::Global))
        .collect()
}

/// Return learnings that apply to the named skill.
pub fn learnings_for_skill<'a>(learnings: &'a [Learning], skill_name: &str) -> Vec<&'a Learning> {
    learnings
        .iter()
        .filter(|l| {
            l.scopes.iter().any(|s| {
                matches!(s, LearningScope::Skill { skill } if skill == skill_name)
            })
        })
        .collect()
}

/// Return learnings that match a channel identifier.
///
/// Matching is exact on the full `channel` string, e.g.
/// `"slack:#sct-internal-dev"`.  Pass `None` to get an empty slice.
pub fn learnings_for_channel<'a>(
    learnings: &'a [Learning],
    channel_id: Option<&str>,
) -> Vec<&'a Learning> {
    let Some(id) = channel_id else {
        return Vec::new();
    };
    learnings
        .iter()
        .filter(|l| {
            l.scopes.iter().any(|s| {
                matches!(s, LearningScope::Channel { channel } if channel == id)
            })
        })
        .collect()
}

/// Return learnings scoped to a specific hook name.
pub fn learnings_for_hook<'a>(learnings: &'a [Learning], hook_name: &str) -> Vec<&'a Learning> {
    learnings
        .iter()
        .filter(|l| {
            l.scopes
                .iter()
                .any(|s| matches!(s, LearningScope::Hook { hook } if hook == hook_name))
        })
        .collect()
}

// ── Prompt rendering ─────────────────────────────────────────────

/// Render a slice of learnings into a system-prompt block.
pub fn learnings_to_prompt(learnings: &[&Learning], header: &str) -> String {
    if learnings.is_empty() {
        return String::new();
    }

    let mut out = format!("## {header}\n\n");
    out.push_str("<learnings>\n");

    for learning in learnings {
        out.push_str("  <learning>\n");
        out.push_str(&format!("    <name>{}</name>\n", xml_escape(&learning.name)));
        out.push_str(&format!(
            "    <description>{}</description>\n",
            xml_escape(&learning.description)
        ));
        if !learning.rules.is_empty() {
            out.push_str("    <rules>\n");
            for rule in &learning.rules {
                out.push_str(&format!(
                    "      <rule>{}</rule>\n",
                    xml_escape(rule.trim())
                ));
            }
            out.push_str("    </rules>\n");
        }
        out.push_str("  </learning>\n");
    }

    out.push_str("</learnings>");
    out
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_learning_dir(base: &Path, name: &str, toml: &str, md: Option<&str>) -> PathBuf {
        let dir = base.join("learnings").join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("LEARNING.toml"), toml).unwrap();
        if let Some(content) = md {
            fs::write(dir.join("LEARNING.md"), content).unwrap();
        }
        dir
    }

    #[test]
    fn load_global_learning() {
        let tmp = tempfile::tempdir().unwrap();
        make_learning_dir(
            tmp.path(),
            "always-link",
            r#"
[learning]
name = "always-link"
description = "Always link commits"
scope = "global"

[[rules]]
content = "Always link commits and PRs in replies."
"#,
            None,
        );

        let learnings = load_learnings(tmp.path());
        assert_eq!(learnings.len(), 1);
        assert_eq!(learnings[0].name, "always-link");
        assert_eq!(learnings[0].scopes, vec![LearningScope::Global]);
        assert_eq!(learnings[0].rules.len(), 1);
    }

    #[test]
    fn load_skill_scoped_learning() {
        let tmp = tempfile::tempdir().unwrap();
        make_learning_dir(
            tmp.path(),
            "draft-pr",
            r#"
[learning]
name = "draft-pr"
description = "Draft PRs first"
scope = "skill:feature-pr"

[[rules]]
content = "Always create PRs as drafts."

[[rules]]
content = "Require human approval before promoting."
"#,
            None,
        );

        let learnings = load_learnings(tmp.path());
        assert_eq!(learnings.len(), 1);
        assert_eq!(
            learnings[0].scopes,
            vec![LearningScope::Skill {
                skill: "feature-pr".into()
            }]
        );
        assert_eq!(learnings[0].rules.len(), 2);
    }

    #[test]
    fn load_multi_scope_learning() {
        let tmp = tempfile::tempdir().unwrap();
        make_learning_dir(
            tmp.path(),
            "channel-ctx",
            r#"
[learning]
name = "channel-ctx"
description = "Channel context"
scopes = ["channel:slack:#dev", "channel:slack:#eng"]

[[rules]]
content = "Assume buysidehub-api context."
"#,
            None,
        );

        let learnings = load_learnings(tmp.path());
        assert_eq!(learnings[0].scopes.len(), 2);
    }

    #[test]
    fn load_learning_with_md_file() {
        let tmp = tempfile::tempdir().unwrap();
        make_learning_dir(
            tmp.path(),
            "with-md",
            r#"
[learning]
name = "with-md"
description = "Has prose"
scope = "global"
"#,
            Some("Long-form prose rule from LEARNING.md."),
        );

        let learnings = load_learnings(tmp.path());
        assert_eq!(learnings[0].rules.len(), 1);
        assert!(learnings[0].rules[0].contains("Long-form prose"));
    }

    #[test]
    fn load_skips_missing_toml() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("learnings").join("no-toml");
        fs::create_dir_all(&dir).unwrap();
        // No LEARNING.toml

        let learnings = load_learnings(tmp.path());
        assert!(learnings.is_empty());
    }

    #[test]
    fn load_skips_no_valid_scopes() {
        let tmp = tempfile::tempdir().unwrap();
        make_learning_dir(
            tmp.path(),
            "bad-scope",
            r#"
[learning]
name = "bad-scope"
description = "Has bad scope"
scope = "invalid:something"

[[rules]]
content = "rule"
"#,
            None,
        );

        let learnings = load_learnings(tmp.path());
        assert!(learnings.is_empty());
    }

    #[test]
    fn filter_global() {
        let global = Learning {
            name: "g".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Global],
            rules: vec![],
            location: None,
        };
        let skill = Learning {
            name: "s".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Skill {
                skill: "foo".into(),
            }],
            rules: vec![],
            location: None,
        };
        let all = vec![global, skill];
        let g = global_learnings(&all);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].name, "g");
    }

    #[test]
    fn filter_by_skill() {
        let l = Learning {
            name: "l".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Skill {
                skill: "feature-pr".into(),
            }],
            rules: vec![],
            location: None,
        };
        let all = vec![l];
        assert_eq!(learnings_for_skill(&all, "feature-pr").len(), 1);
        assert_eq!(learnings_for_skill(&all, "other").len(), 0);
    }

    #[test]
    fn filter_by_channel() {
        let l = Learning {
            name: "c".into(),
            description: "".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Channel {
                channel: "slack:#dev".into(),
            }],
            rules: vec![],
            location: None,
        };
        let all = vec![l];
        assert_eq!(
            learnings_for_channel(&all, Some("slack:#dev")).len(),
            1
        );
        assert_eq!(
            learnings_for_channel(&all, Some("slack:#other")).len(),
            0
        );
        assert_eq!(learnings_for_channel(&all, None).len(), 0);
    }

    #[test]
    fn learnings_to_prompt_empty() {
        assert!(learnings_to_prompt(&[], "Learnings").is_empty());
    }

    #[test]
    fn learnings_to_prompt_renders_xml() {
        let l = Learning {
            name: "test".into(),
            description: "A test".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Global],
            rules: vec!["Do the thing.".into()],
            location: None,
        };
        let prompt = learnings_to_prompt(&[&l], "Active Learnings");
        assert!(prompt.contains("## Active Learnings"));
        assert!(prompt.contains("<name>test</name>"));
        assert!(prompt.contains("<rule>Do the thing.</rule>"));
    }

    #[test]
    fn learnings_to_prompt_escapes_xml() {
        let l = Learning {
            name: "x<y>&z".into(),
            description: "desc".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Global],
            rules: vec!["Use <tool> & check \"q\"".into()],
            location: None,
        };
        let prompt = learnings_to_prompt(&[&l], "Learnings");
        assert!(prompt.contains("<name>x&lt;y&gt;&amp;z</name>"));
        assert!(prompt.contains("&lt;tool&gt; &amp; check &quot;q&quot;"));
    }
}
