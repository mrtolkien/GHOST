# 09 — Discord Bot Interface

## Overview

Discord is the sole user-facing interface for the PoC. The bot runs as part of the
`ghost daemon` process using the serenity library.

## Architecture

The Discord bot is a **thin transport layer**. It:

1. Receives messages from Discord
2. Validates the sender against `discord.allowed_user_id`
3. Passes the message to `SessionChat::chat()`
4. Sends the response back to Discord

It does NOT manage tools, build chat history, or talk to providers directly.

## Session Mapping

Each Discord channel maps to a session. The session ID is derived from the channel ID.
This means:

- DMs with the bot = one session
- Each server channel = a separate session
- The OPERATOR can have multiple concurrent sessions across channels

## Message Handling

### Incoming Messages

- Ignore messages from bots (including self)
- Ignore messages from users other than `discord.allowed_user_id`
- Strip bot mentions from the message text
- Check for `/REBOOT` command (see below)
- Pass cleaned text to `SessionChat::chat()`

### /REBOOT Command

When the OPERATOR sends `/REBOOT`, the Discord handler:

1. Calls `SessionChat::reboot_session()` for the current channel's session
2. Sends a confirmation message (e.g., "Session rebooted. Starting fresh.")
3. Subsequent messages in the channel use the new session

The pre-reboot reflection run is wired in spec 17 — at this step, `/REBOOT` just resets
the session.

### Outgoing Messages — Components v2

GHOST uses Discord's **Components v2** system for rich message rendering instead of
plain text. This was already built in t-koma and should be carried over.

#### Components v2 Flag

Set `1 << 15` (`IS_COMPONENTS_V2`) on the message flags to tell Discord to interpret
components as layout blocks instead of interactive widgets.

#### Component Types

| Type         | ID | Purpose                                                  |
| ------------ | -- | -------------------------------------------------------- |
| TextDisplay  | 10 | Markdown text content (max 4000 chars per block)         |
| MediaGallery | 12 | Image display via `attachment://filename.png`            |
| Separator    | 14 | Horizontal rule/divider with configurable spacing        |
| Container    | 17 | Wrapper with optional accent color (for system messages) |

Max 40 components per message (Discord limit).

#### Markdown-to-Components Converter

Parse the GHOST's markdown response and convert to v2 components:

- **Horizontal rules** (`---`, `***`, `___`) → Separator component
- **Markdown tables** → Rendered as PNG images via SVG pipeline, displayed in
  MediaGallery
- **Regular text** → TextDisplay components, auto-split at 4000 chars on line boundaries
- **Code fences** → Preserved as-is within TextDisplay (tables and HRs inside fences are
  NOT transformed)

#### Table-to-PNG Renderer

Render markdown tables as beautiful inline images using an SVG → PNG pipeline:

- **Pipeline**: Parse table → Build SVG → Rasterize via `resvg` + `tiny_skia`
- **Styling**: Discord dark theme colors (background `#2B2D31`, header `#1E1F22`, accent
  `#5865F2` blurple)
- **Features**: Zebra striping, rounded corners, header accent bar, column separators
- **Typography**: Sans-serif font stack, 14px, bold headers, 2x scale for HiDPI
- **Inline markdown**: Bold, italic, and `code` supported within table cells
- **Attachment**: Generated as `table_N.png`, referenced via MediaGallery component

#### System/Gateway Messages

Wrap system messages in a Container component with accent color:

- Gateway info: `#1283D8` (blue)
- Warnings: `#E03B24` (red)

#### Fallback Chain

1. Try v2 components with attachments
2. On v2 error, fall back to legacy embeds (4096 char limit)
3. Ultimate fallback: plain text messages (2000 char limit)

Log all fallback events — they indicate Discord API issues.

#### Message Splitting

When a response exceeds 40 components:

- Group into multiple messages
- Each message gets up to 40 components
- Preserve component order

### Tool Loop Extension

When `SessionChat::chat()` returns `StopReason::MaxIterations`, the GHOST has hit its
tool call cap (default: 25). The Discord handler should:

1. Send the partial response to the channel
2. Ask the OPERATOR if the GHOST should continue (e.g., "I've hit 25 tool iterations.
   Should I continue?")
3. If the OPERATOR confirms, call `SessionChat::chat()` again in the same session — the
   history picks up where it left off

### Error Handling

- Provider errors → send a user-friendly error message to the channel
- Rate limits → inform the user with the retry-after duration
- Tool errors → handled internally by the chat loop, user sees the final response

## Bot Setup

```rust
pub async fn start_discord(config: &Config, session_chat: Arc<SessionChat>) -> Result<()> {
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(&token, intents)
        .event_handler(GhostHandler {
            session_chat,
            allowed_user_id: config.discord.allowed_user_id.clone(),
        })
        .await?;

    client.start().await?;
    Ok(())
}
```

## Outbound Messaging (DiscordSender)

The heartbeat and reflection subsystems (spec 17) need to send unsolicited messages to
the OPERATOR. Extract an `Arc<DiscordSender>` from the bot that other subsystems can
use:

```rust
pub struct DiscordSender {
    http: Arc<Http>,
}

impl DiscordSender {
    /// Send a message to a channel, using the full v2 component rendering pipeline.
    pub async fn send_to_channel(&self, channel_id: ChannelId, content: &str) -> Result<()>;
}
```

The session ID maps to a Discord channel ID (session mapping above), so heartbeat knows
which channel to send to.

## Typing Indicator

Show the typing indicator while the GHOST is processing:

```rust
// Start typing when we receive a message
let typing = channel_id.start_typing(&ctx.http);
// typing is automatically stopped when dropped
```

## Attachments

For the PoC, handle text-based attachments:

- `.txt`, `.md`, `.rs`, `.py`, `.js`, etc. → extract text content and append to the
  message
- Other file types → mention the filename but don't process content
- Images → mention that image support is not yet available

## Config

```toml
[discord]
enabled = true
allowed_user_id = "123456789012345678"
```

```bash
DISCORD_BOT_TOKEN=...
```

## Observability

```rust
#[tracing::instrument(skip_all, fields(
    user_id = %msg.author.id,
    channel_id = %msg.channel_id,
    message_len = msg.content.len(),
))]
async fn message(&self, ctx: Context, msg: Message) { ... }
```

## Acceptance Criteria

- Bot connects to Discord and shows as online
- Messages from `allowed_user_id` get responses
- Messages from other users are silently ignored
- Long responses are split at 2000-char boundaries
- Typing indicator shows during processing
- Text attachments are extracted and included in the message
- Each channel maps to a separate session
- All Discord events produce tracing spans
- Bot handles reconnection gracefully (serenity handles this automatically)
- Components v2 renders markdown tables as PNG images
- Horizontal rules render as Separator components
- Fallback to legacy embeds/plain text on v2 errors
- `/REBOOT` resets the session and confirms to the OPERATOR
- Tool loop cap prompts the OPERATOR to continue
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-gateway/src/discord/markdown.rs` — Markdown-to-Components v2 converter.
  Handles table detection, HR detection, code fence tracking, text splitting. Directly
  reusable.
- `t-koma-gateway/src/discord/components_v2.rs` — Component builder functions
  (text_display, media_gallery, separator, container). Directly reusable.
- `t-koma-gateway/src/discord/table_image.rs` — SVG-based table-to-PNG renderer with
  Discord dark theme styling. Directly reusable.
- `t-koma-gateway/src/discord/send.rs` — Message sending orchestration with v2/legacy
  fallback chain. Directly reusable pattern.
- `t-koma-gateway/src/discord/bot.rs` — Serenity event handler, message filtering,
  attachment handling. Adapt for single-operator model.
- `t-koma-gateway/src/discord/interactions.rs` — Button/modal handlers. Not needed for
  PoC but useful reference for later.
