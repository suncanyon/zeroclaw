# Design Proposal: Learnings System

**Status:** Draft  
**Author:** Sun Canyon Tech  
**Target version:** post-0.3.x

---

## 1. Problem Statement

ZeroClaw already has three layers of behavioral control:

| Layer | What it is | Where it lives |
|---|---|---|
| Identity | Who the agent *is* | `SOUL.md`, `AGENTS.md`, AiEOS profile |
| Skills | What the agent *can do* | `workspace/skills/<name>/SKILL.md` |
| SOPs | Triggered procedures the agent *executes* | `workspace/sops/<name>/SOP.toml` + `SOP.md` |

There is no first-class home for **soft behavioral rules** — the kind of team/project/channel-specific guidance that:

- Is not core to identity ("always link commits" is a workflow preference, not a personality trait)
- Is not a capability ("when working on PRs, create a draft first and get human approval before promoting" is *how* to use the skill, not the skill itself)
- Varies by team, project, or channel ("in `#sct-internal-dev`, assume requests relate to `buysidehub-api` unless stated otherwise")
- May or may not apply depending on context ("the draft-PR rule only matters when the `feature-pr` skill is active")

Today these rules live in `AGENTS.md` or skill `SKILL.md` files, which creates two problems:
1. **Coupling** — workflow preferences pollute identity/capability definitions, making both harder to maintain
2. **Context blindness** — rules that only apply in certain channels or at certain hook points are always present, adding noise to every prompt

**Learnings** fill this gap: a dedicated, scoped, opt-in layer of behavioral context injected only when relevant.

---

## 2. Concept

A **learning** is a named bundle of behavioral rules with one or more *scopes* that determine when those rules are injected into the agent's context.

### 2.1 Scope Model

| Scope | Injection point | Example use case |
|---|---|---|
| `global` | Every system prompt | "Always link commits and PRs in replies" |
| `skill:<name>` | Alongside the named skill in the prompt | "When using `feature-pr`, create draft PRs first" |
| `channel:<id>` | When the inbound message comes from a matching channel | "In `#sct-internal-dev`, assume `buysidehub-api` context" |
| `hook:<name>` | At the named lifecycle hook | "Before any tool call, prefer read-only operations" |

A single learning can carry multiple scopes (e.g. `skill:feature-pr` + `skill:gh-issues` for a "draft PR" rule that applies to both PR-creating skills).

### 2.2 What Learnings Are Not

- **Not hard rules.** Hard rules belong in `AGENTS.md` / identity. Learnings are *advisory*: they influence behavior without blocking it.
- **Not skill definitions.** Skills define capabilities. Learnings define how to exercise those capabilities for a specific team or workflow.
- **Not SOPs.** SOPs are triggered, step-by-step procedures with execution modes. Learnings are ambient context.

---

## 3. On-Disk Format

Learnings live at `<workspace>/learnings/<name>/`:

```
workspace/
  learnings/
    draft-pr-first/
      LEARNING.toml     ← required
      LEARNING.md       ← optional (long-form prose, appended as a rule)
    sct-dev-channel/
      LEARNING.toml
```

### 3.1 LEARNING.toml

Minimal (single scope):

```toml
[learning]
name        = "draft-pr-first"
description = "Always create PRs as drafts before promoting to review-ready"
version     = "0.1.0"
scope       = "skill:feature-pr"

[[rules]]
content = "Always create PRs as drafts first. Do not promote to non-draft without explicit human approval."

[[rules]]
content = "If a second-agent reviewer is configured, request their review on the draft before promoting."
```

Multi-scope (applies to multiple skills):

```toml
[learning]
name        = "draft-pr-first"
description = "Draft PR gate for all PR-creating skills"
scopes      = ["skill:feature-pr", "skill:gh-issues"]

[[rules]]
content = "Create PRs as drafts first. Wait for explicit approval before marking ready for review."
```

Channel-scoped:

```toml
[learning]
name        = "sct-dev-channel-context"
description = "Contextual defaults for #sct-internal-dev"
scope       = "channel:slack:#sct-internal-dev"

[[rules]]
content = "When a request is ambiguous, assume it relates to the buysidehub-api repository unless the user specifies otherwise."

[[rules]]
content = "Always link PRs, commits, and issues by URL — do not describe changes without a link."
```

Hook-scoped:

```toml
[learning]
name        = "tool-call-caution"
description = "Prefer read-only operations before mutating anything"
scope       = "hook:before_prompt_build"

[[rules]]
content = "Before executing any mutating operation, confirm you understand the full scope of the change."
```

### 3.2 LEARNING.md (optional)

If present, the full Markdown content is appended as an additional rule. Useful for long-form guidance that's awkward in TOML string syntax.

---

## 4. Architecture

### 4.1 Module: `src/learnings/`

New module parallel to `src/skills/` and `src/sop/`:

```
src/learnings/
  mod.rs        ← load_learnings(), filter helpers, learnings_to_prompt()
  types.rs      ← Learning, LearningScope, LearningManifest
```

**Key types:**

```rust
pub enum LearningScope {
    Global,
    Skill { skill: String },
    Channel { channel: String },
    Hook { hook: String },
}

pub struct Learning {
    pub name: String,
    pub description: String,
    pub version: String,
    pub scopes: Vec<LearningScope>,
    pub rules: Vec<String>,
    // ...
}
```

**Filter helpers:**

```rust
pub fn global_learnings(learnings: &[Learning]) -> Vec<&Learning>
pub fn learnings_for_skill(learnings: &[Learning], skill_name: &str) -> Vec<&Learning>
pub fn learnings_for_channel(learnings: &[Learning], channel_id: Option<&str>) -> Vec<&Learning>
pub fn learnings_for_hook(learnings: &[Learning], hook_name: &str) -> Vec<&Learning>
```

### 4.2 Prompt Injection — `src/agent/prompt.rs`

`PromptContext` gains two new fields:

```rust
pub learnings: &'a [Learning],
pub active_channel: Option<&'a str>,
```

Two new `PromptSection` implementations are added to `SystemPromptBuilder::with_defaults()`:

| Section | Position in prompt | What it injects |
|---|---|---|
| `GlobalLearningsSection` | After `SkillsSection` | All learnings with `scope = Global` |
| `ChannelLearningsSection` | After `GlobalLearningsSection` | Learnings matching `active_channel` |

**Skill-scoped learnings** are injected inside `SkillsSection` itself: when rendering each skill's `<instructions>` block, the section also looks up `learnings_for_skill(learnings, skill.name)` and appends matching rules inline. This keeps skill + its behavioral rules co-located in the prompt for maximum model attention.

### 4.3 Hook Injection — `src/hooks/builtin/learnings.rs`

`LearningsHookHandler` implements `HookHandler` with `priority = -10` (runs after standard hooks):

```rust
// Appends hook:before_prompt_build learnings to the assembled prompt string
async fn before_prompt_build(&self, mut prompt: String) -> HookResult<String>

// Pass-through (hook:before_tool_call learnings already in the system prompt)
async fn before_tool_call(&self, name: String, args: Value) -> HookResult<(String, Value)>
```

The handler is registered in the `HookRunner` at agent startup when learnings are present.

### 4.4 Config — `src/config/schema.rs`

New `[learnings]` section:

```toml
[learnings]
enabled = true         # default: true
dir     = "~/custom/learnings"  # default: <workspace>/learnings/
```

### 4.5 Agent Wiring — `src/agent/agent.rs`

- `Agent` struct gains `learnings: Vec<Learning>`
- `AgentBuilder` gains `.learnings(Vec<Learning>)` setter
- `build_system_prompt()` delegates to `build_system_prompt_with_channel(None)`
- New `build_system_prompt_with_channel(active_channel: Option<&str>)` passes `active_channel` + `learnings` into `PromptContext`
- The gateway / daemon request path passes the inbound channel identifier when calling `build_system_prompt_with_channel`

---

## 5. Lifecycle: Message → Prompt

```
1. Inbound message arrives (Slack, Telegram, etc.)
   └─ Channel identifier extracted: e.g. "slack:#sct-internal-dev"

2. HookRunner::run_on_message_received()
   └─ LearningsHookHandler: no-op here (channel id flows into prompt build instead)

3. Agent::build_system_prompt_with_channel("slack:#sct-internal-dev")
   │
   ├─ IdentitySection          → SOUL.md, AGENTS.md, AiEOS
   ├─ ToolsSection             → registered tools
   ├─ SafetySection            → hard safety rules
   ├─ SkillsSection            → skills + skill-scoped learnings inline
   ├─ GlobalLearningsSection   → learnings[scope=Global]
   ├─ ChannelLearningsSection  → learnings[scope=Channel("slack:#sct-internal-dev")]
   ├─ WorkspaceSection
   ├─ DateTimeSection
   ├─ RuntimeSection
   └─ ChannelMediaSection

4. HookRunner::run_before_prompt_build(prompt)
   └─ LearningsHookHandler::before_prompt_build()
      → appends learnings[scope=Hook("before_prompt_build")]

5. LLM call with fully assembled prompt
```

---

## 6. CLI

Future `zeroclaw learnings` subcommands (not in this PR):

```
zeroclaw learnings list           # list all loaded learnings + their scopes
zeroclaw learnings validate       # validate LEARNING.toml files
zeroclaw learnings show <name>    # show rules for a specific learning
```

---

## 7. Example: Full Workflow

**Scenario:** Jake's team wants:
1. Draft PRs before promoting (skill rule)
2. Auto-assume `buysidehub-api` in `#sct-internal-dev` (channel rule)
3. A global rule to always link commits

**workspace/learnings/always-link/LEARNING.toml:**
```toml
[learning]
name = "always-link"
description = "Always link code changes"
scope = "global"

[[rules]]
content = "When you've pushed commits or opened PRs, always provide the direct GitHub link in your reply. Never just say 'I updated file X'."
```

**workspace/learnings/draft-pr-first/LEARNING.toml:**
```toml
[learning]
name = "draft-pr-first"
description = "Draft PR gate"
scopes = ["skill:feature-pr", "skill:gh-issues"]

[[rules]]
content = "Always create PRs as drafts. Do not promote to non-draft without explicit human approval or a second-agent review."
```

**workspace/learnings/sct-dev-context/LEARNING.toml:**
```toml
[learning]
name = "sct-dev-context"
description = "Context for #sct-internal-dev"
scope = "channel:slack:#sct-internal-dev"

[[rules]]
content = "When a request is ambiguous, assume it relates to the buysidehub-api repository."
```

**What the model sees** when a message arrives in `#sct-internal-dev` and the `feature-pr` skill is loaded:

```xml
<!-- In SkillsSection, after feature-pr instructions -->
<learnings>
  <learning>
    <name>draft-pr-first</name>
    <rules>
      <rule>Always create PRs as drafts. Do not promote...</rule>
    </rules>
  </learning>
</learnings>

<!-- GlobalLearningsSection -->
<learnings>
  <learning>
    <name>always-link</name>
    <rules>
      <rule>When you've pushed commits or opened PRs, always provide...</rule>
    </rules>
  </learning>
</learnings>

<!-- ChannelLearningsSection -->
## Channel Context (slack:#sct-internal-dev)
<learnings>
  <learning>
    <name>sct-dev-context</name>
    <rules>
      <rule>When a request is ambiguous, assume it relates to buysidehub-api.</rule>
    </rules>
  </learning>
</learnings>
```

---

## 8. Open Questions / Future Work

- **Skill-scoped injection timing:** Currently skill-scoped learnings require the skill to appear in the loaded skills list. A future improvement could lazy-load learnings when a skill is dynamically selected via the `Compact` injection mode.
- **Agent-to-agent learnings:** A second-agent reviewer pattern (e.g. "draft PR must be approved by both a human and an agent before promoting") could be expressed as a learning with a `requires_confirmation: true` analog — worth exploring as a learning attribute.
- **Learning inheritance:** A `parent` field to inherit rules from another learning (DRY for multi-team setups).
- **`zeroclaw learnings` CLI subcommand** for listing, validating, and showing active learnings.
- **Learning priority** — analogous to `HookHandler::priority()`, allowing teams to define ordering when multiple learnings apply.
