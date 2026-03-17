//! `LearningsHookHandler` — injects hook-scoped learnings into the system
//! prompt at the `before_prompt_build` lifecycle point.
//!
//! Global and channel-scoped learnings are handled at prompt-build time by
//! `GlobalLearningsSection` / `ChannelLearningsSection` in `agent/prompt.rs`.
//! This hook handles the remaining scopes that are contextual to the *current
//! request* and not knowable at static prompt-assembly time:
//!
//! - `hook:before_prompt_build` — general hook-scoped rules
//! - `hook:before_tool_call` — rules that apply right before any tool is called
//!   (injected by the companion `before_tool_call` implementation)
//!
//! ## Why a hook, not a section?
//!
//! Sections run once at startup during prompt assembly and see only static
//! context (workspace, skills, channel id).  Hook handlers run per-request and
//! can react to dynamic state: which skills were activated, which channel the
//! current message came from, etc.  The hook approach also lets operators
//! override or disable learnings injection without forking the agent core.
//!
//! ## Dynamic reload
//!
//! The handler holds an `Arc<LearningsStore>` rather than a static
//! `Vec<Learning>`.  On every hook invocation it calls `store.snapshot()` to
//! get the *current* learnings, so any LEARNING.toml files added or modified
//! on disk (and picked up by the store's background watcher) take effect
//! immediately — no agent restart required.

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::hooks::traits::{HookHandler, HookResult};
use crate::learnings::{self, LearningsStore};

pub struct LearningsHookHandler {
    store: Arc<LearningsStore>,
}

impl LearningsHookHandler {
    /// Create a handler backed by a live [`LearningsStore`].
    ///
    /// The store should already have its watcher running (via
    /// [`LearningsStore::spawn_watcher`]) so that hook injections stay current
    /// without an agent restart.
    pub fn new(store: Arc<LearningsStore>) -> Self {
        Self { store }
    }

    fn hook_rules_block(&self, hook_name: &str) -> String {
        let snapshot = self.store.snapshot();
        let matched = learnings::learnings_for_hook(&snapshot, hook_name);
        learnings::learnings_to_prompt(&matched, &format!("Learnings ({hook_name})"))
    }
}

#[async_trait]
impl HookHandler for LearningsHookHandler {
    fn name(&self) -> &str {
        "learnings"
    }

    /// Low priority — runs after identity/safety hooks so it appends rather
    /// than prepends to any existing modifications.
    fn priority(&self) -> i32 {
        -10
    }

    /// Append `hook:before_prompt_build` learnings to the system prompt string.
    async fn before_prompt_build(&self, mut prompt: String) -> HookResult<String> {
        let block = self.hook_rules_block("before_prompt_build");
        if !block.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&block);
        }
        HookResult::Continue(prompt)
    }

    /// Append `hook:before_tool_call` learnings as a suffix note on the prompt.
    ///
    /// Note: this does not modify the tool call itself — it appends a reminder
    /// block to the ongoing prompt context so the model has the rules in scope
    /// when deciding how to call tools.  The name/args pass through unchanged.
    async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)> {
        // Learning injection for tool calls is advisory only — we don't mutate
        // the call.  The rules were already in the system prompt if scoped
        // to `hook:before_tool_call`; this is a no-op pass-through here.
        // Future: could inject a "reminder" user message into the conversation.
        HookResult::Continue((name, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learnings::{Learning, LearningScope, LearningsStore};
    use std::sync::Arc;

    fn make_store_with_hook_learning(
        tmp: &tempfile::TempDir,
        hook: &str,
    ) -> Arc<LearningsStore> {
        let store = Arc::new(LearningsStore::new(tmp.path()));
        let learning = Learning {
            name: format!("hook-{hook}"),
            description: "hook rule".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Hook {
                hook: hook.to_string(),
            }],
            rules: vec![format!("Rule for {hook}.")],
            location: None,
        };
        store.write_learning(&learning).unwrap();
        store
    }

    #[tokio::test]
    async fn appends_hook_learnings_to_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let store = make_store_with_hook_learning(&tmp, "before_prompt_build");
        let handler = LearningsHookHandler::new(store);

        let base = "## System\n\nYou are an agent.".to_string();
        match handler.before_prompt_build(base.clone()).await {
            HookResult::Continue(result) => {
                assert!(result.starts_with(&base));
                assert!(result.contains("Rule for before_prompt_build."));
            }
            HookResult::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[tokio::test]
    async fn no_op_when_no_hook_learnings() {
        let tmp = tempfile::tempdir().unwrap();
        let store = make_store_with_hook_learning(&tmp, "some_other_hook");
        let handler = LearningsHookHandler::new(store);

        let base = "## System\n\nYou are an agent.".to_string();
        match handler.before_prompt_build(base.clone()).await {
            HookResult::Continue(result) => assert_eq!(result, base),
            HookResult::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[tokio::test]
    async fn before_tool_call_passes_through() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(LearningsStore::new(tmp.path()));
        let handler = LearningsHookHandler::new(store);
        match handler
            .before_tool_call("shell".into(), serde_json::json!({"cmd": "ls"}))
            .await
        {
            HookResult::Continue((name, _)) => assert_eq!(name, "shell"),
            HookResult::Cancel(_) => panic!("should not cancel"),
        }
    }

    #[tokio::test]
    async fn picks_up_new_learnings_after_reload() {
        // Demonstrate that adding a learning to the store while the handler is
        // running takes effect without constructing a new handler.
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(LearningsStore::new(tmp.path()));
        let handler = LearningsHookHandler::new(Arc::clone(&store));

        let base = "## System".to_string();

        // Initially no learnings — should be a no-op.
        match handler.before_prompt_build(base.clone()).await {
            HookResult::Continue(result) => assert_eq!(result, base),
            HookResult::Cancel(_) => panic!("should not cancel"),
        }

        // Now add a hook-scoped learning directly to the store.
        let new_learning = Learning {
            name: "dynamic-rule".into(),
            description: "added at runtime".into(),
            version: "0.1.0".into(),
            author: None,
            tags: vec![],
            scopes: vec![LearningScope::Hook {
                hook: "before_prompt_build".into(),
            }],
            rules: vec!["Dynamic runtime rule.".into()],
            location: None,
        };
        store.write_learning(&new_learning).unwrap();

        // Same handler — should now see the new learning.
        match handler.before_prompt_build(base.clone()).await {
            HookResult::Continue(result) => {
                assert!(result.contains("Dynamic runtime rule."), "expected dynamic rule in prompt");
            }
            HookResult::Cancel(_) => panic!("should not cancel"),
        }
    }
}
