use serenity::builder::{CreateAttachment, CreateEmbed, CreateMessage};
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tracing::{debug, trace, warn};

use super::components_v2::{
    container, group_into_v2_messages, send_v2_message, send_v2_message_suppress_embeds,
    text_display,
};
use super::markdown;

pub const DISCORD_MESSAGE_LIMIT: usize = 2000;
const DISCORD_EMBED_DESC_LIMIT: usize = 4096;
pub const GATEWAY_EMBED_COLOR: u32 = 0xFF_5C_6E; // Neon Maroon
pub const WARNING_EMBED_COLOR: u32 = 0xFF_88_33; // Neon Peach

// ---------------------------------------------------------------------------
// v2 assistant text (GHOST responses)
// ---------------------------------------------------------------------------

/// Send GHOST assistant text using Components v2 markdown rendering.
/// Falls back to legacy plain text on v2 errors.
#[tracing::instrument(name = "send message", skip_all, fields(channel_id = %channel_id, content_len = content.len()))]
pub async fn send_assistant_v2(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    Box::pin(send_assistant_v2_with_suffix(http, channel_id, content, &[])).await
}

/// Send GHOST assistant text with extra v2 components appended to the
/// last message chunk (used for statusline).
#[tracing::instrument(name = "send message", skip_all, fields(channel_id = %channel_id, content_len = content.len()))]
pub async fn send_assistant_v2_with_suffix(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
    suffix: &[serde_json::Value],
) -> serenity::Result<()> {
    let markdown::MarkdownComponents {
        mut components,
        attachments,
    } = markdown::markdown_to_v2_components(content);

    debug!(
        component_count = components.len(),
        attachment_count = attachments.len(),
        "v2 markdown conversion complete"
    );

    if components.is_empty() {
        warn!(
            content_len = content.len(),
            "Ghost response produced no v2 components, falling back to legacy"
        );
        return send_plain_text(http, channel_id, content).await;
    }

    // Append suffix components (e.g. statusline) to the component list
    components.extend_from_slice(suffix);

    let chunks = group_into_v2_messages(components);
    trace!(chunk_count = chunks.len(), "sending v2 message chunks");

    for (i, chunk) in chunks.iter().enumerate() {
        let files = attachments_for_chunk(chunk, &attachments);
        trace!(
            chunk_index = i,
            components_in_chunk = chunk.len(),
            file_count = files.len(),
            "sending v2 chunk"
        );
        if let Err(e) = send_v2_message_suppress_embeds(http, channel_id, chunk, files).await {
            warn!(
                chunk_index = i,
                error = %e,
                "v2 message failed, falling back to legacy"
            );
            return send_plain_text(http, channel_id, content).await;
        }
    }

    debug!("v2 assistant message sent");
    Ok(())
}

/// Collect `CreateAttachment` items referenced by `MediaGallery` components
/// in a chunk.
fn attachments_for_chunk(
    chunk: &[serde_json::Value],
    all: &[markdown::MarkdownAttachment],
) -> Vec<CreateAttachment> {
    let mut files = Vec::new();
    for comp in chunk {
        if comp["type"] != 12 {
            continue;
        }
        let Some(items) = comp["items"].as_array() else {
            continue;
        };
        for item in items {
            let Some(url) = item["media"]["url"].as_str() else {
                continue;
            };
            let Some(name) = url.strip_prefix("attachment://") else {
                continue;
            };
            if let Some(att) = all.iter().find(|a| a.filename == name) {
                files.push(CreateAttachment::bytes(att.data.clone(), &att.filename));
            }
        }
    }
    files
}

// ---------------------------------------------------------------------------
// v2 gateway messages (system/info messages)
// ---------------------------------------------------------------------------

/// Send a gateway system message as a v2 Container with accent color.
/// Falls back to embed on v2 failure. No action rows in GHOST PoC.
#[tracing::instrument(name = "send message", skip_all, fields(channel_id = %channel_id))]
pub async fn send_gateway_v2(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
    color: Option<u32>,
) -> serenity::Result<()> {
    let inner = vec![text_display(&format!("**GHOST**\n\n{content}"))];
    let accent = color.unwrap_or(GATEWAY_EMBED_COLOR);
    let message_components = vec![container(inner, Some(accent))];

    match send_v2_message(http, channel_id, &message_components, Vec::new()).await {
        Ok(_) => Ok(()),
        Err(e) => {
            warn!("v2 gateway message failed, falling back to embed: {e}");
            send_gateway_embed_http(http, channel_id, content, color).await
        }
    }
}

// ---------------------------------------------------------------------------
// Legacy fallbacks
// ---------------------------------------------------------------------------

/// Split a message into chunks that fit within Discord's 2000-char limit.
///
/// Tries to split at line boundaries first, preserving code fence state across
/// chunks. Falls back to hard character splitting as a last resort.
pub fn split_discord_message(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_MESSAGE_LIMIT {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut open_fence = false;

    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        let current_len = current.chars().count();
        if current_len + line_len > DISCORD_MESSAGE_LIMIT && !current.is_empty() {
            if open_fence {
                current.push_str("\n```");
            }
            chunks.push(current);
            current = String::new();
            if open_fence {
                current.push_str("```\n");
            }
        }
        current.push_str(line);
        if line.trim_start().starts_with("```") {
            open_fence = !open_fence;
        }
    }

    if !current.is_empty() {
        if open_fence {
            current.push_str("\n```");
        }
        chunks.push(current);
    }

    if chunks.len() > 1 {
        return chunks;
    }

    // Hard-split by char as last resort
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for ch in content.chars() {
        if current_len + 1 > DISCORD_MESSAGE_LIMIT {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push(ch);
        current_len += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Send plain text messages, splitting at the Discord 2000-char limit.
#[tracing::instrument(name = "send message", skip_all, fields(channel_id = %channel_id, content_len = content.len()))]
pub async fn send_plain_text(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
) -> serenity::Result<()> {
    let chunks = split_discord_message(content);
    debug!(
        chunk_count = chunks.len(),
        "sending legacy plain-text message"
    );
    for chunk in chunks {
        channel_id.say(http, chunk).await?;
    }
    debug!("legacy message sent");
    Ok(())
}

/// Split text for Discord embed descriptions (4096-char limit).
/// Same strategy as `split_discord_message` but with a higher limit.
pub fn split_discord_embed_description(content: &str) -> Vec<String> {
    if content.chars().count() <= DISCORD_EMBED_DESC_LIMIT {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if current_len + line_len > DISCORD_EMBED_DESC_LIMIT && !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push_str(line);
        current_len += line_len;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.len() > 1 {
        return chunks;
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for ch in content.chars() {
        if current_len + 1 > DISCORD_EMBED_DESC_LIMIT {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }
        current.push(ch);
        current_len += 1;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

/// Send a gateway embed (legacy fallback for v2 failures).
#[tracing::instrument(name = "send message", skip_all, fields(channel_id = %channel_id))]
pub async fn send_gateway_embed_http(
    http: &Http,
    channel_id: ChannelId,
    content: &str,
    color: Option<u32>,
) -> serenity::Result<()> {
    let chunks = split_discord_embed_description(content);
    for (index, chunk) in chunks.iter().enumerate() {
        let title = if index == 0 { "GHOST" } else { "GHOST (CONT.)" };
        let embed = CreateEmbed::new()
            .title(title)
            .description(chunk.clone())
            .color(color.unwrap_or(GATEWAY_EMBED_COLOR));

        let msg = CreateMessage::new().embed(embed);
        channel_id.send_message(http, msg).await?;
    }
    Ok(())
}
