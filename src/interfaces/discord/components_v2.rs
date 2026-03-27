/// Discord Components v2 types for rich message layout.
///
/// Serenity 0.12 doesn't have builder types for Components v2, so we
/// construct raw JSON payloads and send them via `Http::send_message()`.
/// The v2 flag (`1 << 15` in message flags) tells Discord to interpret the
/// `components` array as layout blocks rather than legacy action rows.
///
/// For messages **with file attachments** (table images), serenity's
/// `Http::send_message()` is bypassed in favour of a direct reqwest
/// multipart upload. Serenity 0.12 sets `CreateAttachment.id = 0` on
/// every file (the field is `pub(crate)`) so all uploads share the
/// `files[0]` multipart key and Discord only sees the first one.
/// Building the form ourselves lets us assign sequential IDs and include
/// the `attachments` JSON array that Discord requires.
use serenity::builder::CreateAttachment;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tracing::warn;

/// Components v2 message flag (IS_COMPONENTS_V2 = 1 << 15).
const V2_FLAG: u64 = 1 << 15;

/// Suppress URL auto-embeds (SUPPRESS_EMBEDS = 1 << 2).
const SUPPRESS_EMBEDS: u64 = 1 << 2;

/// Maximum components per v2 message.
pub const MAX_V2_COMPONENTS: usize = 40;

/// Maximum characters in a single TextDisplay content.
pub const TEXT_DISPLAY_LIMIT: usize = 4000;

/// Maximum total displayable text across all components in a single message.
/// Discord rejects v2 messages exceeding this with:
/// "Components displayable text size exceeds maximum size of 4000"
const MESSAGE_TEXT_LIMIT: usize = 4000;

/// Build a `TextDisplay` component (type 10).
pub fn text_display(content: &str) -> serde_json::Value {
    serde_json::json!({
        "type": 10,
        "content": content,
    })
}

/// Build a `Separator` component (type 14).
pub fn separator(divider: bool) -> serde_json::Value {
    serde_json::json!({
        "type": 14,
        "divider": divider,
        "spacing": 1,
    })
}

/// Build a `MediaGallery` component (type 12) referencing an attachment by
/// filename.
pub fn media_gallery(attachment_filename: &str) -> serde_json::Value {
    serde_json::json!({
        "type": 12,
        "items": [{
            "media": {
                "url": format!("attachment://{attachment_filename}")
            }
        }]
    })
}

/// Build a `Button` component (type 2).
///
/// `style`: 1=Primary (blue), 2=Secondary (grey), 3=Success (green), 4=Danger (red)
pub fn button(label: &str, custom_id: &str, style: u8) -> serde_json::Value {
    serde_json::json!({
        "type": 2,
        "style": style,
        "label": label,
        "custom_id": custom_id,
    })
}

/// Build an `ActionRow` component (type 1) containing buttons.
pub fn action_row(components: Vec<serde_json::Value>) -> serde_json::Value {
    serde_json::json!({
        "type": 1,
        "components": components,
    })
}

/// Build a `Container` component (type 17) wrapping inner components.
pub fn container(
    components: Vec<serde_json::Value>,
    accent_color: Option<u32>,
) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "type": 17,
        "components": components,
    });
    if let Some(color) = accent_color {
        obj["accent_color"] = serde_json::json!(color);
    }
    obj
}

/// Send a Components v2 message on a channel.
///
/// `components` is the top-level v2 component array. If it exceeds
/// `MAX_V2_COMPONENTS`, only the first 40 are sent (Discord limit).
/// `attachments` are file uploads (e.g. table images) referenced by
/// components.
pub async fn send_v2_message(
    http: &Http,
    channel_id: ChannelId,
    components: &[serde_json::Value],
    attachments: Vec<CreateAttachment>,
) -> serenity::Result<serenity::model::channel::Message> {
    send_v2(http, channel_id, V2_FLAG, components, attachments).await
}

/// Send a Components v2 message with URL auto-embeds suppressed.
pub async fn send_v2_message_suppress_embeds(
    http: &Http,
    channel_id: ChannelId,
    components: &[serde_json::Value],
    attachments: Vec<CreateAttachment>,
) -> serenity::Result<serenity::model::channel::Message> {
    send_v2(
        http,
        channel_id,
        V2_FLAG | SUPPRESS_EMBEDS,
        components,
        attachments,
    )
    .await
}

/// Shared implementation for v2 message sends.
///
/// When `attachments` is empty, delegates to serenity's `Http::send_message`.
/// When files are present, builds a raw reqwest multipart form so that each
/// file gets a unique `files[N]` key and the JSON payload includes the
/// `attachments` array that Discord requires for `attachment://` resolution.
async fn send_v2(
    http: &Http,
    channel_id: ChannelId,
    flags: u64,
    components: &[serde_json::Value],
    attachments: Vec<CreateAttachment>,
) -> serenity::Result<serenity::model::channel::Message> {
    let capped = if components.len() > MAX_V2_COMPONENTS {
        warn!(
            "v2 message has {} components, capping at {}",
            components.len(),
            MAX_V2_COMPONENTS
        );
        &components[..MAX_V2_COMPONENTS]
    } else {
        components
    };

    if attachments.is_empty() {
        let payload = serde_json::json!({
            "flags": flags,
            "components": capped,
        });
        return http.send_message(channel_id, Vec::new(), &payload).await;
    }

    // Build attachments metadata for the JSON payload.
    let attachments_meta: Vec<serde_json::Value> = attachments
        .iter()
        .enumerate()
        .map(|(i, f)| serde_json::json!({"id": i, "filename": &f.filename}))
        .collect();

    let payload = serde_json::json!({
        "flags": flags,
        "components": capped,
        "attachments": attachments_meta,
    });

    // Build multipart form with correctly indexed file parts.
    let mut form = reqwest::multipart::Form::new().text("payload_json", payload.to_string());

    for (i, file) in attachments.into_iter().enumerate() {
        let part = reqwest::multipart::Part::bytes(file.data)
            .file_name(file.filename)
            .mime_str("image/png")
            .expect("image/png is a valid MIME");
        form = form.part(format!("files[{i}]"), part);
    }

    let url = format!("https://discord.com/api/v10/channels/{channel_id}/messages");

    let response = reqwest::Client::new()
        .post(&url)
        .header("Authorization", http.token())
        .multipart(form)
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, "v2 multipart upload failed");
            serenity::Error::Other("v2 multipart request failed")
        })?;

    let status = response.status();
    let body = response.bytes().await.map_err(|e| {
        warn!(error = %e, "v2 multipart response read failed");
        serenity::Error::Other("v2 multipart response read failed")
    })?;

    if status.is_success() {
        serde_json::from_slice(&body).map_err(serenity::Error::from)
    } else {
        let body_str = String::from_utf8_lossy(&body);
        warn!(status = %status, body = %body_str, "v2 multipart upload rejected");
        Err(serenity::Error::Other("v2 multipart upload rejected"))
    }
}

/// Edit an existing Components v2 message.
pub async fn edit_v2_message(
    http: &Http,
    channel_id: ChannelId,
    message_id: serenity::model::id::MessageId,
    components: &[serde_json::Value],
) -> serenity::Result<serenity::model::channel::Message> {
    let capped = if components.len() > MAX_V2_COMPONENTS {
        &components[..MAX_V2_COMPONENTS]
    } else {
        components
    };

    let payload = serde_json::json!({
        "flags": V2_FLAG,
        "components": capped,
    });

    http.edit_message(channel_id, message_id, &payload, Vec::new())
        .await
}

/// Group a flat list of v2 components into message-sized chunks that respect
/// both the component count limit (`MAX_V2_COMPONENTS`) and the total
/// displayable text limit (`MESSAGE_TEXT_LIMIT`).
pub fn group_into_v2_messages(components: Vec<serde_json::Value>) -> Vec<Vec<serde_json::Value>> {
    let mut chunks: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut current: Vec<serde_json::Value> = Vec::new();
    let mut current_text_len: usize = 0;

    for comp in components {
        let comp_text_len = displayable_text_len(&comp);

        // Start a new chunk if adding this component would exceed either limit,
        // unless the current chunk is empty (a single component that exceeds
        // the text limit must go in its own chunk regardless).
        if !current.is_empty()
            && (current.len() >= MAX_V2_COMPONENTS
                || current_text_len + comp_text_len > MESSAGE_TEXT_LIMIT)
        {
            chunks.push(std::mem::take(&mut current));
            current_text_len = 0;
        }

        current_text_len += comp_text_len;
        current.push(comp);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    if chunks.is_empty() {
        chunks.push(Vec::new());
    }

    chunks
}

/// Estimate the displayable text length of a component tree.
///
/// Counts `content` fields on TextDisplay (type 10) and `label` fields on
/// Buttons (type 2), recursing into `components` arrays for containers and
/// action rows.
fn displayable_text_len(comp: &serde_json::Value) -> usize {
    let mut len = 0;

    // TextDisplay content, Button label
    if let Some(s) = comp["content"].as_str() {
        len += s.len();
    }
    if let Some(s) = comp["label"].as_str() {
        len += s.len();
    }

    // Recurse into nested components (Container, ActionRow)
    if let Some(children) = comp["components"].as_array() {
        for child in children {
            len += displayable_text_len(child);
        }
    }

    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_splits_on_text_size() {
        // Two TextDisplay components of 2500 chars each — combined 5000 > 4000.
        let a = text_display(&"a".repeat(2500));
        let b = text_display(&"b".repeat(2500));
        let chunks = group_into_v2_messages(vec![a, b]);
        assert_eq!(chunks.len(), 2, "should split into 2 messages");
        assert_eq!(chunks[0].len(), 1);
        assert_eq!(chunks[1].len(), 1);
    }

    #[test]
    fn group_keeps_small_components_together() {
        let a = text_display(&"a".repeat(1000));
        let b = text_display(&"b".repeat(1000));
        let c = text_display(&"c".repeat(1000));
        let chunks = group_into_v2_messages(vec![a, b, c]);
        // 1000+1000+1000 = 3000 < 4000, all fit in one message
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 3);
    }

    #[test]
    fn group_splits_on_component_count() {
        let comps: Vec<_> = (0..50).map(|_| separator(false)).collect();
        let chunks = group_into_v2_messages(comps);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 40);
        assert_eq!(chunks[1].len(), 10);
    }

    #[test]
    fn group_single_oversized_component_goes_alone() {
        // A single component exceeding the text limit must still be sent.
        let big = text_display(&"x".repeat(5000));
        let small = text_display("hello");
        let chunks = group_into_v2_messages(vec![big, small]);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn group_empty_input() {
        let chunks = group_into_v2_messages(vec![]);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].is_empty());
    }

    #[test]
    fn displayable_text_len_text_display() {
        let td = text_display("hello world");
        assert_eq!(displayable_text_len(&td), 11);
    }

    #[test]
    fn displayable_text_len_container() {
        let inner = text_display("abc");
        let c = container(vec![inner], None);
        assert_eq!(displayable_text_len(&c), 3);
    }

    #[test]
    fn displayable_text_len_separator() {
        let s = separator(true);
        assert_eq!(displayable_text_len(&s), 0);
    }
}
