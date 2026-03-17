# Slack Setup Guide

This guide walks you through connecting ZeroClaw to Slack from scratch. By the end you'll have a bot that reads messages in your workspace and replies in the configured channel(s).

---

## Prerequisites

- A Slack workspace where you have permission to install apps
- ZeroClaw installed (`zeroclaw --version` should work)
- A provider and API key already configured (`zeroclaw onboard` or `config.toml`)

---

## Step 1: Create a Slack App

1. Go to [https://api.slack.com/apps](https://api.slack.com/apps)
2. Click **Create New App** → **From scratch**
3. Name your app (e.g. `ZeroClaw`) and select your workspace
4. Click **Create App**

---

## Step 2: Add Bot Token Scopes

1. In the left sidebar click **OAuth & Permissions**
2. Scroll to **Scopes → Bot Token Scopes**
3. Add all of the following scopes:

| Scope | Required for |
|-------|-------------|
| `chat:write` | Sending replies |
| `channels:history` | Reading public channel messages |
| `groups:history` | Reading private channel messages |
| `im:history` | Reading DM messages |
| `mpim:history` | Reading multi-party DM messages |
| `channels:read` | Channel discovery (wildcard / all-channel mode) |
| `groups:read` | Private channel discovery |
| `users:read` | Resolving sender display names |

---

## Step 3: Install the App to Your Workspace

1. Still in **OAuth & Permissions**, click **Install to Workspace**
2. Review the permissions and click **Allow**
3. Copy the **Bot User OAuth Token** — it starts with `xoxb-`

Keep this token safe. It grants the bot access to your workspace.

---

## Step 4: (Optional but Recommended) Enable Socket Mode

Socket Mode uses a persistent WebSocket connection instead of REST API polling. It gives lower latency (~instant vs ~3 s) and does not require a public inbound URL.

1. In the left sidebar click **Socket Mode**
2. Toggle **Enable Socket Mode** to ON
3. Click **Generate an app-level token**
4. Name the token (e.g. `socket-mode`) and add the `connections:write` scope
5. Click **Generate** and copy the token — it starts with `xapp-`

**Also required for Socket Mode:** Go to **Event Subscriptions**, enable events, and subscribe to these **Bot Events**:

- `message.channels`
- `message.groups`
- `message.im`
- `message.mpim`

Click **Save Changes**.

> Without these event subscriptions the bot will connect but receive no messages.

---

## Step 5: Invite the Bot to Channels

In Slack, open each channel you want the bot to monitor and type:

```
/invite @YourBotName
```

The bot only receives messages from channels it has been invited to. For DMs it is available automatically.

---

## Step 6: Find Your Channel ID (Optional)

If you want to restrict the bot to one specific channel:

1. Right-click the channel name in the Slack sidebar
2. Click **View channel details**
3. Scroll to the bottom — the **Channel ID** starts with `C` (public) or `G` (private group)

You can also find it in the channel URL: `https://app.slack.com/client/TEAM_ID/C0123456789`

To find your own user ID (for the `allowed_users` list):

1. Click your profile picture → **Profile**
2. Click **⋮ (More)** → **Copy member ID**

---

## Step 7: Configure ZeroClaw

### Option A — Interactive Wizard (recommended for first-time setup)

```bash
zeroclaw onboard --channels-only
```

Select **Slack** from the channel menu. The wizard prompts for your bot token, validates it live against `auth.test`, then optionally asks for your app token, channel ID, and allowed users.

### Option B — Edit `config.toml` Directly

Open `~/.zeroclaw/config.toml` and add:

```toml
[channels_config.slack]
bot_token = "xoxb-your-bot-token-here"
app_token = "xapp-your-app-token-here"   # optional — enables Socket Mode
channel_id = "C0123456789"               # optional — omit or set "*" for all channels
allowed_users = ["*"]                    # "*" = everyone; or list specific user IDs
interrupt_on_new_message = false         # cancel active request when same sender sends again
```

#### Field Reference

| Field | Type | Required | Default | Notes |
|-------|------|----------|---------|-------|
| `bot_token` | string | ✅ | — | Must start with `xoxb-` |
| `app_token` | string | ❌ | `none` | Must start with `xapp-`; enables Socket Mode |
| `channel_id` | string | ❌ | `none` | Single channel restriction; omit or `"*"` = all accessible |
| `allowed_users` | string array | ❌ | `[]` | Slack user IDs; `"*"` = allow everyone; **empty = deny all** |
| `interrupt_on_new_message` | bool | ❌ | `false` | Cancel in-flight request if same sender sends again |

> ⚠️ **Important:** `allowed_users = []` (the out-of-the-box default) means **no one can trigger the bot**. Set it to `["*"]` to allow everyone, or list specific Slack user IDs.

---

## Step 8: Start the Daemon

For first-time setup, run in the foreground so you can see what's happening:

```bash
zeroclaw daemon --log-level debug
```

You should see output like:

```
Slack: auth.test OK — bot=mybot (U0123456789), workspace=My Team (https://myteam.slack.com/)
Slack: starting in Socket Mode (app_token present).
  ↳ Channels: C0123456789
Slack Socket Mode: WebSocket connected and ready (channels: C0123456789)
```

If you see `polling mode` instead of `Socket Mode`, your `app_token` is missing or empty. If you see an `auth.test failed` error, your `bot_token` is wrong — regenerate it in the Slack app dashboard.

Once verified, run as a background service:

```bash
# Install and start the system service
zeroclaw service install
zeroclaw service start

# Check it's running
zeroclaw service status
zeroclaw doctor
```

---

## Step 9: Test It

Send a message in a channel the bot is in (or DM it directly). You should receive a reply.

### Confirming end-to-end

```bash
# Check health
zeroclaw doctor

# Follow live logs (service mode)
journalctl --user -u zeroclaw.service -f        # Linux systemd
tail -f ~/.zeroclaw/logs/daemon.stdout.log       # macOS / Linux launchd
sudo tail -f /var/log/zeroclaw/error.log         # Linux OpenRC
```

---

## Listen Mode Reference

ZeroClaw automatically selects the listen mode based on your config:

| Mode | When active | Latency | Notes |
|------|-------------|---------|-------|
| **Socket Mode** | `app_token` is set | ~instant | Recommended for production |
| **Polling** | No `app_token` | ~3 s | Simpler setup; polls `conversations.history` every 3 s |

---

## Troubleshooting

### Startup errors

| Log message | Cause | Fix |
|-------------|-------|-----|
| `auth.test failed: invalid_auth` | `bot_token` wrong or revoked | Regenerate bot token in Slack app dashboard |
| `auth.test failed: account_inactive` | App or workspace deactivated | Contact your Slack admin |
| `apps.connections.open failed: socket_mode_not_enabled` | Socket Mode not turned on | Enable at **Socket Mode** in your Slack app settings |
| `apps.connections.open failed: missing_scope` or `no_permission` | App token missing `connections:write` | Regenerate app token and add `connections:write` scope |
| `apps.connections.open failed: invalid_auth` | `app_token` wrong or revoked | Ensure it starts with `xapp-`; regenerate in app dashboard |

### Bot receives nothing

| Symptom | Likely cause | Fix |
|---------|-------------|-----|
| Bot online but no responses | `allowed_users` empty | Add user IDs or set `allowed_users = ["*"]` |
| Not receiving group messages | Not invited to channel | Run `/invite @YourBotName` in the channel |
| Missing event subscriptions | Socket Mode events not configured | Subscribe to `message.channels`, `message.groups`, `message.im`, `message.mpim` under Event Subscriptions |

### Debug logging

```bash
# Real-time debug output via CLI flag
zeroclaw daemon --log-level debug

# Scoped to Slack module only (less noise from other systems)
RUST_LOG=zeroclaw::channels::slack=debug zeroclaw daemon

# Via RUST_LOG env var
RUST_LOG=debug zeroclaw daemon
```

---

## Quick Reference

```bash
# First-time interactive setup
zeroclaw onboard --channels-only

# Start in foreground with debug output
zeroclaw daemon --log-level debug

# Install and run as a service
zeroclaw service install && zeroclaw service start

# Health check
zeroclaw doctor

# View service logs (Linux systemd)
journalctl --user -u zeroclaw.service -f

# View service logs (macOS)
tail -f ~/.zeroclaw/logs/daemon.stdout.log
```
