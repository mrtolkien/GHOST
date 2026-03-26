//! Live tests for Discord message delivery.
//!
//! These tests send real messages to the OPERATOR via Discord DM.
//! Requires `DISCORD_BOT_TOKEN` and `DISCORD_USER_ID` env vars.
//!
//! Run with: `cargo test --features live-tests --test discord_live`
#![cfg(feature = "live-tests")]

use serenity::http::Http;
use serenity::model::id::UserId;

use ghost::interfaces::discord::DiscordSender;

fn discord_env() -> (String, u64) {
    let token = std::env::var("DISCORD_BOT_TOKEN")
        .expect("DISCORD_BOT_TOKEN must be set for Discord live tests");
    // Support the .env typo (DISORD_USER_ID) as well as the correct name.
    let user_id: u64 = std::env::var("DISCORD_USER_ID")
        .or_else(|_| std::env::var("DISORD_USER_ID"))
        .expect("DISCORD_USER_ID must be set for Discord live tests")
        .parse()
        .expect("DISCORD_USER_ID must be a u64");
    (token, user_id)
}

/// Open (or reuse) a DM channel with the operator.
async fn dm_channel_id(http: &Http, user_id: u64) -> u64 {
    let channel = UserId::new(user_id)
        .create_dm_channel(http)
        .await
        .expect("failed to open DM channel with operator");
    channel.id.get()
}

/// Sends a message containing **multiple markdown tables** and asserts that
/// the v2 path succeeds (tables rendered as PNG, embeds suppressed).
///
/// Before the fix, serenity uploaded every attachment as `files[0]` because
/// `CreateAttachment::bytes` sets `id: 0`. Discord would reject the second
/// table image with "The referenced attachment was not found", causing a
/// fallback to legacy plain-text (no table rendering, URL embeds shown).
#[tokio::test]
async fn multi_table_message_sends_successfully() {
    let (token, user_id) = discord_env();
    let sender = DiscordSender::from_token(&token);
    let channel_id = dm_channel_id(sender.http(), user_id).await;

    let content = "\
## Multi-Table Test

First table:

| Language | Typing | Speed |
|----------|--------|-------|
| Rust | static | fast |
| Python | dynamic | slow |
| Go | static | fast |

Some text between tables with a [link](https://example.com) that should not embed.

Second table:

| Feature | Supported |
|---------|-----------|
| Tables as PNG | yes |
| Embed suppression | yes |
| Multiple attachments | yes |

Third table:

| Step | Description |
|------|-------------|
| 1 | Detect markdown tables |
| 2 | Render each as PNG |
| 3 | Upload with unique file IDs |

If you see this as rendered images, the fix works.";

    sender
        .send_to_channel(channel_id, content)
        .await
        .expect("v2 message with multiple tables should succeed");
}
