# Logging & Diagnostics Guide

How to surface, filter, and interpret ZeroClaw logs on macOS and Linux — whether running interactively, as a system service, or in a containerized environment.

---

## Log Levels

ZeroClaw uses structured logging via the [`tracing`](https://docs.rs/tracing) crate. Five levels from least to most verbose:

| Level | What's included | Use when |
|-------|----------------|----------|
| `error` | Fatal errors only | Production — minimal noise |
| `warn` | Errors + recoverable warnings | Production — catch problems early |
| `info` | **(default)** Startup, connections, per-message activity | Normal operation |
| `debug` | Per-request details, retry logic, auth probes, API call details | Diagnosing connectivity issues |
| `trace` | Extremely verbose; internal state transitions, raw message payloads | Deep debugging only |

---

## Controlling Log Verbosity

### Via `--log-level` flag (recommended)

Works on all subcommands (`daemon`, `doctor`, `onboard`, etc.):

```bash
zeroclaw daemon --log-level debug
zeroclaw daemon --log-level trace
zeroclaw daemon --log-level warn   # quieter; only warnings and errors
```

### Via `RUST_LOG` environment variable

Supports Rust's [`EnvFilter`](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html) syntax, which allows per-module filtering:

```bash
# All modules at debug level
RUST_LOG=debug zeroclaw daemon

# Scoped to the Slack channel module only (much less noise)
RUST_LOG=zeroclaw::channels::slack=debug zeroclaw daemon

# Scoped to all channel code
RUST_LOG=zeroclaw::channels=debug zeroclaw daemon

# Mix: warn for everything, debug for channels
RUST_LOG=warn,zeroclaw::channels=debug zeroclaw daemon
```

**Priority:** `--log-level` flag → `RUST_LOG` env var → default (`info`)

### Useful scoped filters by subsystem

| Goal | Filter |
|------|--------|
| Slack only | `zeroclaw::channels::slack=debug` |
| All channels | `zeroclaw::channels=debug` |
| Agent/LLM calls | `zeroclaw::agent=debug` |
| Tool execution | `zeroclaw::tools=debug` |
| Cron/scheduler | `zeroclaw::cron=debug` |
| Gateway/HTTP | `zeroclaw::gateway=debug` |
| All ZeroClaw (noisy) | `zeroclaw=debug` |

---

## Where Logs Go

### Foreground (interactive)

Logs go to **stdout/stderr** in your terminal:

```bash
# Watch live
zeroclaw daemon --log-level debug

# Capture to a file while watching
zeroclaw daemon 2>&1 | tee ~/zeroclaw.log

# Capture at info level, search offline
RUST_LOG=info zeroclaw daemon 2>&1 | tee /tmp/zeroclaw.log
grep -i "slack\|error\|warn" /tmp/zeroclaw.log
```

---

### macOS — launchd service

ZeroClaw installs as a launchd user agent. The plist is written to:

```
~/Library/LaunchAgents/com.zeroclaw.daemon.plist
```

Log files are written to:

```
~/.zeroclaw/logs/daemon.stdout.log   ← main log stream (stdout)
~/.zeroclaw/logs/daemon.stderr.log   ← error stream (stderr)
```

#### Viewing logs

```bash
# Follow live (stdout)
tail -f ~/.zeroclaw/logs/daemon.stdout.log

# Follow both streams combined
tail -f ~/.zeroclaw/logs/daemon.stdout.log ~/.zeroclaw/logs/daemon.stderr.log

# Search for errors
grep -i "error\|warn" ~/.zeroclaw/logs/daemon.stdout.log

# Recent 100 lines
tail -100 ~/.zeroclaw/logs/daemon.stdout.log

# Filter by channel
grep -i "slack" ~/.zeroclaw/logs/daemon.stdout.log
```

#### Enabling debug logging for the service

Add an `EnvironmentVariables` entry to the plist:

```bash
# Stop the service first
zeroclaw service stop

# Edit the plist
nano ~/Library/LaunchAgents/com.zeroclaw.daemon.plist
```

Add inside the top-level `<dict>`:

```xml
<key>EnvironmentVariables</key>
<dict>
  <key>RUST_LOG</key>
  <string>debug</string>
</dict>
```

Then reload:

```bash
launchctl unload ~/Library/LaunchAgents/com.zeroclaw.daemon.plist
launchctl load -w ~/Library/LaunchAgents/com.zeroclaw.daemon.plist
```

Alternatively, reinstall the service after setting `RUST_LOG` in your shell environment — the plist will inherit the value if `zeroclaw service install` is run with it set.

#### Service status

```bash
# Check if the launchd job is loaded
launchctl list | grep zeroclaw

# Detailed job info
launchctl print gui/$(id -u)/com.zeroclaw.daemon

# Service status via CLI
zeroclaw service status
```

---

### Linux — systemd user service

Unit file location:

```
~/.config/systemd/user/zeroclaw.service
```

#### Viewing logs via journalctl

```bash
# Follow live
journalctl --user -u zeroclaw.service -f

# Last 100 lines
journalctl --user -u zeroclaw.service -n 100

# Since last boot
journalctl --user -u zeroclaw.service -b

# Since a specific time
journalctl --user -u zeroclaw.service --since "1 hour ago"
journalctl --user -u zeroclaw.service --since "2025-01-01 09:00:00"

# Filter by severity (error and above)
journalctl --user -u zeroclaw.service -p err

# Search (pipe to grep or ripgrep)
journalctl --user -u zeroclaw.service -f | grep -i slack
journalctl --user -u zeroclaw.service | rg "error|warn"

# Export to a file
journalctl --user -u zeroclaw.service --since "24 hours ago" > ~/zeroclaw-logs.txt
```

#### Enabling debug logging for the service

Create a systemd override:

```bash
systemctl --user edit zeroclaw.service
```

This opens an editor. Add:

```ini
[Service]
Environment=RUST_LOG=debug
```

Save and reload:

```bash
systemctl --user daemon-reload
systemctl --user restart zeroclaw.service
```

To use `--log-level` instead, check the current `ExecStart` in the unit file and append the flag:

```bash
cat ~/.config/systemd/user/zeroclaw.service
# Look for ExecStart=...
```

Edit the unit file (or override) to add `--log-level debug`:

```ini
[Service]
ExecStart=/path/to/zeroclaw daemon --log-level debug
```

#### Service management

```bash
zeroclaw service status
zeroclaw service start
zeroclaw service stop
zeroclaw service restart

# Or via systemctl directly
systemctl --user status zeroclaw.service
systemctl --user restart zeroclaw.service
```

---

### Linux — OpenRC (Alpine / Artix / Gentoo)

Init script: `/etc/init.d/zeroclaw`

Log files:

```
/var/log/zeroclaw/access.log   ← stdout (main log stream)
/var/log/zeroclaw/error.log    ← stderr
```

#### Viewing logs

```bash
# Follow live (errors)
sudo tail -f /var/log/zeroclaw/error.log

# Follow both streams
sudo tail -f /var/log/zeroclaw/access.log /var/log/zeroclaw/error.log

# Search
sudo grep -i "error\|slack\|warn" /var/log/zeroclaw/error.log

# Recent entries
sudo tail -100 /var/log/zeroclaw/access.log
```

#### Enabling debug logging for the service

Edit the init script:

```bash
sudo nano /etc/init.d/zeroclaw
```

Change the `command_args` line to add `--log-level debug`:

```bash
# Before
command_args="--config-dir /etc/zeroclaw daemon"

# After
command_args="--config-dir /etc/zeroclaw daemon --log-level debug"
```

Or add `RUST_LOG` as an environment variable in the script:

```bash
# Add near the top of the init script, after the header block:
export RUST_LOG="debug"
```

Then restart:

```bash
sudo rc-service zeroclaw restart
```

#### Service status

```bash
sudo rc-service zeroclaw status
```

---

## Structured Observability Backends

Beyond plaintext log output, ZeroClaw supports several structured observability backends configured in `config.toml`:

```toml
[observability]
backend = "log"                                  # none | log | verbose | prometheus | otel
otel_endpoint = "http://localhost:4318"          # OTLP HTTP endpoint (otel backend only)
otel_service_name = "zeroclaw"
runtime_trace_mode = "rolling"                   # none | rolling | full
runtime_trace_path = "state/runtime-trace.jsonl" # relative to workspace_dir
runtime_trace_max_entries = 200
```

### Backend options

| Backend | Output destination | Best for |
|---------|-------------------|----------|
| `none` / `noop` | Discarded | Minimal overhead; production where you have external tracing |
| `log` | `tracing::info!` structured lines | Log aggregators: Datadog, Loki, CloudWatch, Splunk |
| `verbose` | Human-readable progress to stderr | Local development and debugging |
| `prometheus` | Metrics exposed at gateway `/metrics` endpoint | Grafana, Prometheus server |
| `otel` / `opentelemetry` / `otlp` | OTLP HTTP to configured endpoint | Jaeger, Honeycomb, Grafana Tempo, Datadog OTLP receiver |

### Prometheus metrics

When `backend = "prometheus"`, these metrics are available at the gateway `/metrics` endpoint:

| Metric | Type | Description |
|--------|------|-------------|
| `zeroclaw_agent_starts_total` | Counter | Total agent sessions started |
| `zeroclaw_llm_requests_total` | Counter | Total LLM API calls made |
| `zeroclaw_tokens_input_total` | Counter | Input tokens consumed |
| `zeroclaw_tokens_output_total` | Counter | Output tokens generated |
| `zeroclaw_tool_calls_total` | Counter | Tool invocations |
| `zeroclaw_channel_messages_total` | Counter | Messages received across all channels |
| `zeroclaw_errors_total` | Counter | Error events |
| `zeroclaw_agent_duration_seconds` | Histogram | Agent turn duration |
| `zeroclaw_tool_duration_seconds` | Histogram | Tool execution duration |
| `zeroclaw_request_latency_seconds` | Histogram | End-to-end request latency |
| `zeroclaw_sessions_active` | Gauge | Current active sessions |
| `zeroclaw_queue_depth` | Gauge | Pending message queue depth |

### OpenTelemetry (OTLP)

```toml
[observability]
backend = "otel"
otel_endpoint = "http://localhost:4318"
otel_service_name = "zeroclaw"
```

Compatible collectors: Jaeger, Honeycomb, Grafana Tempo, Datadog Agent (OTLP receiver), New Relic, OpenTelemetry Collector.

Quick local test with Jaeger all-in-one:

```bash
docker run -d -p 16686:16686 -p 4318:4318 jaegertracing/all-in-one
# Then open http://localhost:16686
```

Note: the OTLP exporter uses a **blocking HTTP client** to avoid Tokio reactor panics in background batch threads.

---

## Runtime Traces (JSONL)

ZeroClaw can write a local structured trace file of all agent activity:

```toml
[observability]
runtime_trace_mode = "rolling"         # rolling = keep last N entries; full = append forever
runtime_trace_path = "state/runtime-trace.jsonl"
runtime_trace_max_entries = 200
```

The trace file is written relative to `workspace_dir`, is mutex-locked on every write, and has `0o600` permissions (owner-readable only).

### Querying traces via CLI

```bash
# Show last 20 events
zeroclaw doctor traces --limit 20

# Filter by event type
zeroclaw doctor traces --event tool_call_result
zeroclaw doctor traces --event llm_response

# Search content
zeroclaw doctor traces --event tool_call_result --contains "error"

# Find a specific trace by ID
zeroclaw doctor traces --id <trace-id>
```

### Trace entry format (JSONL)

Each line is a JSON object:

```json
{
  "id": "550e8400-e29b-41d4-a716-446655440000",
  "timestamp": "2025-01-01T09:00:00.000Z",
  "event_type": "tool_call_result",
  "channel": "slack",
  "provider": "openrouter",
  "model": "claude-3-5-sonnet",
  "turn_id": "7b2f3a1c-...",
  "success": true,
  "message": "tool executed successfully",
  "payload": {}
}
```

---

## Health Check & Doctor

```bash
# Full diagnostic report
zeroclaw doctor

# Check daemon and channel component liveness
zeroclaw doctor | grep -E "daemon|channel|slack"
```

Doctor stale thresholds (time since last heartbeat before flagging as stale):

| Component | Stale after |
|-----------|------------|
| Daemon | 30 seconds |
| Scheduler | 120 seconds |
| Channel | 300 seconds |

The health registry tracks per-component: status (`starting` / `ok` / `error`), last seen time, last error, and restart count.

---

## Quick Reference

| Goal | Command |
|------|---------|
| Debug in foreground | `zeroclaw daemon --log-level debug` |
| Scoped Slack debug | `RUST_LOG=zeroclaw::channels::slack=debug zeroclaw daemon` |
| Follow service logs — Linux systemd | `journalctl --user -u zeroclaw.service -f` |
| Follow service logs — macOS | `tail -f ~/.zeroclaw/logs/daemon.stdout.log` |
| Follow service logs — OpenRC | `sudo tail -f /var/log/zeroclaw/error.log` |
| Capture all logs to file | `zeroclaw daemon 2>&1 \| tee ~/zeroclaw.log` |
| View recent traces | `zeroclaw doctor traces --limit 20` |
| Full health check | `zeroclaw doctor` |
| Service status | `zeroclaw service status` |

---

## Platform Log File Summary

| Platform | Init system | stdout log | stderr log |
|----------|------------|-----------|-----------|
| macOS | launchd | `~/.zeroclaw/logs/daemon.stdout.log` | `~/.zeroclaw/logs/daemon.stderr.log` |
| Linux | systemd | `journalctl --user -u zeroclaw.service` | same |
| Linux | OpenRC | `/var/log/zeroclaw/access.log` | `/var/log/zeroclaw/error.log` |
| Any | foreground | stdout (terminal) | stderr (terminal) |
