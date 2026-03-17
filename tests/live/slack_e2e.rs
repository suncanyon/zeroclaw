//! End-to-end tests for the Slack channel integration.
//!
//! Tests are marked `#[ignore]` and skipped unless the required environment
//! variables are present. They make real network calls to the Slack API.
//!
//! # Required environment variables
//!
//! | Variable | Description |
//! |----------|-------------|
//! | `ZEROCLAW_TEST_SLACK_BOT_TOKEN` | Bot OAuth token (`xoxb-...`) |
//!
//! # Optional environment variables
//!
//! | Variable | Description |
//! |----------|-------------|
//! | `ZEROCLAW_TEST_SLACK_APP_TOKEN` | App-level token (`xapp-...`) — enables Socket Mode tests |
//! | `ZEROCLAW_TEST_SLACK_CHANNEL_ID` | Channel ID to post test messages to (e.g. `C0123456789`) |
//! | `ZEROCLAW_TEST_SLACK_USER_ID` | Expected bot user ID; verified against `auth.test` response |
//!
//! # Running
//!
//! ```bash
//! export ZEROCLAW_TEST_SLACK_BOT_TOKEN=xoxb-...
//! export ZEROCLAW_TEST_SLACK_APP_TOKEN=xapp-...        # optional
//! export ZEROCLAW_TEST_SLACK_CHANNEL_ID=C0123456789   # optional
//!
//! # Run all Slack E2E tests
//! cargo test --test test_live slack -- --ignored --nocapture
//!
//! # Run a single test
//! cargo test --test test_live slack_auth_test_succeeds -- --ignored --nocapture
//! ```

use std::time::Duration;
use tokio::time::timeout;
use zeroclaw::channels::slack::SlackChannel;
use zeroclaw::channels::traits::{Channel, SendMessage};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Read a required env var. Returns `None` (and prints a skip message) if absent/empty.
/// Tests call `require_env!` then return early on `None` rather than failing.
fn get_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

macro_rules! require_env {
    ($name:literal) => {
        match get_env($name) {
            Some(val) => val,
            None => {
                eprintln!(
                    "[SKIP] {} not set — skipping this Slack E2E test.\n\
                     Set {} to a valid value to run it.",
                    $name, $name
                );
                return;
            }
        }
    };
}

/// Build a minimal reqwest client for direct Slack API calls.
fn slack_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("failed to build HTTP client")
}

// ── Auth ──────────────────────────────────────────────────────────────────────

/// auth.test succeeds, workspace name is present, and user_id is non-empty.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN"]
async fn slack_auth_test_succeeds() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");

    let resp = slack_client()
        .get("https://slack.com/api/auth.test")
        .bearer_auth(&bot_token)
        .send()
        .await
        .expect("auth.test network request failed");

    assert!(
        resp.status().is_success(),
        "auth.test returned HTTP {}",
        resp.status()
    );

    let body: serde_json::Value = resp
        .json()
        .await
        .expect("auth.test response is not valid JSON");

    assert_eq!(
        body.get("ok"),
        Some(&serde_json::Value::Bool(true)),
        "auth.test ok=false — Slack error: {:?}\n\
         Hint: regenerate your bot_token at https://api.slack.com/apps",
        body.get("error")
    );

    let user_id = body
        .get("user_id")
        .and_then(|v| v.as_str())
        .expect("auth.test response missing user_id field");

    assert!(
        !user_id.is_empty(),
        "user_id in auth.test response is empty"
    );

    let team = body
        .get("team")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    eprintln!("[OK] auth.test — user_id={user_id}, workspace={team}");

    // If caller provided their expected user ID, verify it matches.
    if let Some(expected) = get_env("ZEROCLAW_TEST_SLACK_USER_ID") {
        assert_eq!(
            user_id, expected,
            "auth.test user_id ({user_id}) does not match ZEROCLAW_TEST_SLACK_USER_ID ({expected})"
        );
    }
}

// ── Token format validation ───────────────────────────────────────────────────

/// Bot token must start with xoxb-.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN"]
async fn slack_bot_token_has_correct_prefix() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    assert!(
        bot_token.starts_with("xoxb-"),
        "bot_token must start with `xoxb-`, got prefix: `{}`\n\
         Hint: copy the Bot User OAuth Token, not the app token.",
        &bot_token[..bot_token.len().min(12)]
    );
}

/// App token (if provided) must start with xapp-.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN"]
async fn slack_app_token_has_correct_prefix_when_provided() {
    let _bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");

    if let Some(app_token) = get_env("ZEROCLAW_TEST_SLACK_APP_TOKEN") {
        assert!(
            app_token.starts_with("xapp-"),
            "app_token must start with `xapp-`, got prefix: `{}`\n\
             Hint: copy the App-Level Token from Socket Mode settings.",
            &app_token[..app_token.len().min(12)]
        );
        eprintln!("[OK] app_token prefix is xapp-");
    } else {
        eprintln!("[SKIP] ZEROCLAW_TEST_SLACK_APP_TOKEN not set — prefix check skipped");
    }
}

// ── SlackChannel health check ─────────────────────────────────────────────────

/// health_check() returns true with valid credentials.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN"]
async fn slack_health_check_passes_with_valid_credentials() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    let app_token = get_env("ZEROCLAW_TEST_SLACK_APP_TOKEN");

    let channel = SlackChannel::new(bot_token, app_token, None, vec![], vec!["*".to_string()]);

    let healthy = timeout(Duration::from_secs(20), channel.health_check())
        .await
        .expect("health_check timed out after 20s");

    assert!(
        healthy,
        "health_check() returned false — verify bot_token is valid and not revoked"
    );
    eprintln!("[OK] health_check passed");
}

/// health_check() returns false with a clearly invalid token.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN (negative test — no real token needed)"]
async fn slack_health_check_fails_with_invalid_bot_token() {
    // This test does NOT require env vars — it uses a deliberately wrong token.
    let channel = SlackChannel::new(
        "xoxb-invalid-token-000000000000".to_string(),
        None,
        None,
        vec![],
        vec!["*".to_string()],
    );

    let healthy = timeout(Duration::from_secs(20), channel.health_check())
        .await
        .expect("health_check timed out after 20s");

    assert!(
        !healthy,
        "health_check() should return false for a clearly invalid token"
    );
    eprintln!("[OK] health_check correctly failed for invalid token");
}

// ── Required OAuth scopes ─────────────────────────────────────────────────────

/// Bot token has channels:read scope — needed for channel discovery.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN"]
async fn slack_bot_token_has_channels_read_scope() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");

    let resp = slack_client()
        .get("https://slack.com/api/conversations.list")
        .bearer_auth(&bot_token)
        .query(&[("limit", "1"), ("exclude_archived", "true")])
        .send()
        .await
        .expect("conversations.list network request failed");

    assert!(resp.status().is_success(), "HTTP {}", resp.status());

    let body: serde_json::Value = resp.json().await.expect("response is not valid JSON");
    let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("none");

    assert!(
        body.get("ok") == Some(&serde_json::Value::Bool(true)),
        "conversations.list returned ok=false: `{error}`\n\
         Hint: add scopes `channels:read` and `groups:read` in OAuth & Permissions."
    );

    eprintln!("[OK] channels:read scope confirmed");
}

/// Bot token can read channel history — needed for polling mode.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN and ZEROCLAW_TEST_SLACK_CHANNEL_ID"]
async fn slack_bot_token_has_channels_history_scope() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    let channel_id = require_env!("ZEROCLAW_TEST_SLACK_CHANNEL_ID");

    let resp = slack_client()
        .get("https://slack.com/api/conversations.history")
        .bearer_auth(&bot_token)
        .query(&[("channel", channel_id.as_str()), ("limit", "1")])
        .send()
        .await
        .expect("conversations.history network request failed");

    assert!(resp.status().is_success(), "HTTP {}", resp.status());

    let body: serde_json::Value = resp.json().await.expect("response is not valid JSON");
    let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("none");

    assert!(
        body.get("ok") == Some(&serde_json::Value::Bool(true)),
        "conversations.history returned ok=false: `{error}`\n\
         Hint: add scope `channels:history` (or `groups:history` for private channels). \
         Also make sure the bot is invited to channel {channel_id}."
    );

    eprintln!("[OK] channels:history scope confirmed for channel {channel_id}");
}

// ── Socket Mode ───────────────────────────────────────────────────────────────

/// apps.connections.open succeeds and returns a wss:// URL.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_APP_TOKEN"]
async fn slack_socket_mode_url_opens_with_valid_app_token() {
    let _bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    let app_token = require_env!("ZEROCLAW_TEST_SLACK_APP_TOKEN");

    let resp = slack_client()
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth(&app_token)
        .send()
        .await
        .expect("apps.connections.open network request failed");

    assert!(
        resp.status().is_success(),
        "apps.connections.open returned HTTP {}",
        resp.status()
    );

    let body: serde_json::Value = resp.json().await.expect("response is not valid JSON");
    let error = body.get("error").and_then(|v| v.as_str()).unwrap_or("none");

    assert_eq!(
        body.get("ok"),
        Some(&serde_json::Value::Bool(true)),
        "apps.connections.open returned ok=false: `{error}`\n\
         Hint: ensure Socket Mode is enabled at https://api.slack.com/apps \
         and that app_token has the `connections:write` scope."
    );

    let ws_url = body
        .get("url")
        .and_then(|v| v.as_str())
        .expect("apps.connections.open response missing url field");

    assert!(
        ws_url.starts_with("wss://"),
        "WebSocket URL must start with wss://, got: {ws_url}"
    );

    eprintln!(
        "[OK] Socket Mode URL obtained ({}...)",
        &ws_url[..ws_url.len().min(50)]
    );
}

/// apps.connections.open fails with an explicitly wrong app token.
#[tokio::test]
#[ignore = "negative test — no env vars required"]
async fn slack_socket_mode_fails_with_invalid_app_token() {
    let resp = slack_client()
        .post("https://slack.com/api/apps.connections.open")
        .bearer_auth("xapp-invalid-token-000000000000")
        .send()
        .await
        .expect("network request failed");

    let body: serde_json::Value = resp.json().await.expect("response is not valid JSON");

    assert_ne!(
        body.get("ok"),
        Some(&serde_json::Value::Bool(true)),
        "apps.connections.open should fail for an invalid app token"
    );

    eprintln!(
        "[OK] apps.connections.open correctly rejected invalid token — error: {:?}",
        body.get("error")
    );
}

// ── Send message ──────────────────────────────────────────────────────────────

/// Sends a real message to a Slack channel via SlackChannel::send().
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN and ZEROCLAW_TEST_SLACK_CHANNEL_ID"]
async fn slack_send_message_to_channel() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    let channel_id = require_env!("ZEROCLAW_TEST_SLACK_CHANNEL_ID");

    let channel = SlackChannel::new(
        bot_token,
        None,
        Some(channel_id.clone()),
        vec![],
        vec!["*".to_string()],
    );

    let msg = SendMessage::new(
        "🤖 ZeroClaw Slack E2E test — if you see this, the Slack integration is working correctly.",
        &channel_id,
    );

    let result = timeout(Duration::from_secs(20), channel.send(&msg))
        .await
        .expect("send() timed out after 20s");

    result.unwrap_or_else(|e| {
        panic!(
            "send() failed: {e}\n\
             Hint: ensure bot_token has the `chat:write` scope and the bot \
             is invited to channel {channel_id}."
        )
    });

    eprintln!("[OK] Test message sent to channel {channel_id}");
}

/// Sends a threaded reply to an existing message.
#[tokio::test]
#[ignore = "requires ZEROCLAW_TEST_SLACK_BOT_TOKEN and ZEROCLAW_TEST_SLACK_CHANNEL_ID"]
async fn slack_send_threaded_reply() {
    let bot_token = require_env!("ZEROCLAW_TEST_SLACK_BOT_TOKEN");
    let channel_id = require_env!("ZEROCLAW_TEST_SLACK_CHANNEL_ID");

    let channel = SlackChannel::new(
        bot_token,
        None,
        Some(channel_id.clone()),
        vec![],
        vec!["*".to_string()],
    );

    // First, post a root message.
    let root_msg = SendMessage::new("🤖 ZeroClaw E2E — thread parent message.", &channel_id);
    let result = timeout(Duration::from_secs(20), channel.send(&root_msg))
        .await
        .expect("root send() timed out");
    result.expect("root send() failed");

    // Then post a threaded reply. We use a synthetic thread_ts here — in
    // production this would come from the inbound ChannelMessage.thread_ts.
    // The point is to verify the send() call with thread_ts set does not panic
    // or error at the API layer (Slack returns ok=true even for unknown thread_ts).
    let reply = SendMessage::new("🤖 ZeroClaw E2E — thread reply.", &channel_id)
        .in_thread(Some("1000000000.000001".to_string()));

    let result = timeout(Duration::from_secs(20), channel.send(&reply))
        .await
        .expect("thread send() timed out");
    result.expect("thread send() failed");

    eprintln!("[OK] Threaded reply sent to channel {channel_id}");
}

// ── Channel configuration ─────────────────────────────────────────────────────

/// Channel reports the correct name.
#[test]
fn slack_channel_name_is_slack() {
    let channel = SlackChannel::new(
        "xoxb-fake".to_string(),
        None,
        None,
        vec![],
        vec!["*".to_string()],
    );
    assert_eq!(channel.name(), "slack");
}

/// Channel constructed without channel_id operates in wildcard mode (no panic on construction).
#[test]
fn slack_channel_without_id_constructs_successfully() {
    let _channel = SlackChannel::new(
        "xoxb-fake".to_string(),
        None,
        None,
        vec![],
        vec!["*".to_string()],
    );
}

/// Channel constructed with a specific ID constructs successfully.
#[test]
fn slack_channel_with_specific_id_constructs_successfully() {
    let _channel = SlackChannel::new(
        "xoxb-fake".to_string(),
        None,
        Some("C0123456789".to_string()),
        vec![],
        vec!["*".to_string()],
    );
}

/// Channel constructed with app_token reflects Socket Mode intent.
#[test]
fn slack_channel_with_app_token_constructs_successfully() {
    let _channel = SlackChannel::new(
        "xoxb-fake".to_string(),
        Some("xapp-fake".to_string()),
        None,
        vec![],
        vec!["*".to_string()],
    );
}

/// Allowed users list with wildcard constructs without panic.
#[test]
fn slack_channel_wildcard_allowed_users() {
    let _channel = SlackChannel::new(
        "xoxb-fake".to_string(),
        None,
        None,
        vec![],
        vec!["*".to_string()],
    );
}

/// Empty allowed_users list constructs without panic (deny-all mode).
#[test]
fn slack_channel_empty_allowed_users_is_deny_all() {
    // This should construct without panic — the deny-all behavior is
    // enforced at message dispatch time, not construction time.
    let _channel = SlackChannel::new("xoxb-fake".to_string(), None, None, vec![], vec![]);
}
