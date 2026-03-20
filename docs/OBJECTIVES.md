# ZeroClaw (Sun Canyon Fork) — Project Objectives & Guidelines

> Internal document. Drives priorities, architectural decisions, and contribution standards for Sun Canyon's fork of ZeroClaw.

---

## What ZeroClaw Is (For Us)

ZeroClaw is the **agent infrastructure backbone** for Sun Canyon's AI automation work. It's the runtime that schedules jobs, routes messages, delivers outputs, and keeps agents alive across channels. The upstream project optimizes for hardware efficiency and open-source accessibility. Our fork optimizes for **reliability, observability, and production-grade autonomous operation**.

We track upstream closely but move independently when our needs diverge.

---

## Core Objectives

### 1. High-Reliability Automation
Automated surfaces — cron jobs, PR monitors, heartbeats, email watchers — must work **continuously and correctly without human intervention**. A cron job that silently fails for 4 hours is not acceptable. Every automated surface must have:
- Failure detection (consecutive error tracking)
- Self-healing or escalation when failures exceed threshold
- Observable state (structured logs, health metrics)

### 2. Non-AI Health Layer
The agent cannot be its own doctor. We maintain a **non-AI watchdog layer** — deterministic processes that validate config, reset stale state, and escalate via hardcoded channels — separate from the agent runtime. See [Issue #5](https://github.com/suncanyon/zeroclaw/issues/5).

### 3. Minimal Configuration Drift
Config mistakes (wrong channel ID formats, missing env vars, stale state files) are a leading cause of silent failures. The runtime must:
- Validate delivery targets at job registration time, not at execution time
- Surface misconfigurations via `zeroclaw doctor` before they cause failures
- Enforce consistent ID/reference formats (no raw channel IDs without prefixes, etc.)

### 4. Full Observability
Every agent action, cron run, skill execution, and delivery attempt must produce structured, queryable output. We integrate with Elasticsearch/Kibana for log aggregation and APM for tracing. Observability is not optional — if it doesn't produce a log, it didn't happen.

### 5. Skill Reliability
Skills are the primary extension mechanism. They must be:
- Idempotent where possible (safe to re-run on failure)
- Versioned (changes tracked in git, not in-place overwrites)
- Tested before deployment (cron skills especially)
- Documented with `SKILL.md` that includes failure modes and recovery steps

---

## Guiding Principles

### Ship Small, Ship Often
Prefer small, targeted changes over large rewrites. A PR that changes 50 lines and fixes one thing is better than a PR that changes 500 lines and "cleans things up." When in doubt, split.

### Blame the Config, Not the Agent
When automation fails, the first question is: "was the config right?" Invest in config validation, not agent debugging. Agents can be surprisingly reliable when given correct inputs.

### Automate the Automation
Meta-principle: if you find yourself manually fixing a cron job, stale state file, or delivery config more than once, it should be automated. Document it first, then build the fix into the watchdog layer.

### One Source of Truth
Job schedules, channel targets, and skill configs live in the workspace repo (`jenoc-workspace`). No config lives only in a running process or an agent's memory. If it's not in git, it doesn't exist.

### Fail Loudly
Silent failures are worse than noisy failures. A failed job that produces no output and no alert is a trust-eroding event. All failures must produce: a log entry, a consecutive failure counter update, and (above threshold) an alert.

---

## Architecture Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Fork tracking | Track upstream `main`, merge selectively | Preserve upstream improvements (hardware, performance) while adding our reliability layer |
| Watchdog layer | Non-AI process (shell/Rust/systemd) | Agent cannot self-heal; watchdog must be independent |
| Config format | `cron/jobs.json` in workspace repo | Single source of truth, git-tracked, auditable |
| Delivery validation | Pre-flight validation at job registration | Catch `channel:` vs raw ID mismatches before first run |
| State files | Gitignored runtime files in `memory/` | Ephemeral state separate from config; watchdog can reset without touching git |
| Observability | Structured JSON logs → Elasticsearch | Queryable, correlatable, survives agent restarts |
| Escalation channel | Hardcoded webhook/email in watchdog config | Must work even when agent Slack integration is broken |

---

## What We Don't Do

- **We don't merge upstream blindly.** Every upstream merge gets reviewed for conflicts with our reliability layer.
- **We don't let config live only in agent memory.** If an agent "knows" something (a channel ID, a credential, a schedule), it must also be in a config file.
- **We don't accept silent failures.** If a job fails and nothing was logged and nobody was alerted, the fix is not to fix the job — it's to fix the observability layer first.
- **We don't run watchdog logic through the agent.** The watchdog is deterministic and separate. Routing self-healing through the agent creates circular dependency.

---

## Open Issues & Roadmap

| Issue | Priority | Description |
|---|---|---|
| [#5 — Self-healing watchdog](https://github.com/suncanyon/zeroclaw/issues/5) | High | Non-AI watchdog for cron job health, config validation, state reset, escalation |
| Pre-flight config validation | Medium | Validate all delivery targets at job registration, surface errors in `zeroclaw doctor` |
| Structured cron logging | Medium | Emit structured JSON logs for every job run (start, result, duration, errors) |
| Watchdog + `zeroclaw doctor` integration | Medium | Single health view covering agent runtime + watchdog + all scheduled jobs |
| Upstream merge cadence | Low | Establish a quarterly review process for upstream changes |

---

## Contribution Standards (Sun Canyon Fork)

These apply **in addition to** the upstream `CONTRIBUTING.md`:

1. **All commits attributed to Jen** (`jen@suncanyontech.com`) — the agent is the committer, not the human operator.
2. **All repos private by default** — no public forks of internal config or workspace files.
3. **PRs require human merge approval** — the agent opens PRs but never merges them.
4. **Cron job changes must include a test run** — any change to `cron/jobs.json` should be validated with a manual trigger before it's considered done.
5. **Skill changes must be announced** — when skills or AGENTS.md are modified, the agent recites the diff in conversation so the human can review.

---

*Last updated: 2026-03-20 | Maintained by: Jen (AI) / Jake Ferrante (Sun Canyon Tech)*
