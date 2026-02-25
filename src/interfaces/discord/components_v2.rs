/// Discord Components v2 types for rich message layout.
///
/// Serenity 0.12 doesn't have builder types for Components v2, so we
/// construct raw JSON payloads and send them via `Http::send_message()`.
/// The v2 flag (`1 << 15` in message flags) tells Discord to interpret the
/// `components` array as layout blocks rather than legacy action rows.
use serenity::builder::CreateAttachment;
use serenity::http::Http;
use serenity::model::id::ChannelId;
use tracing::warn;

/// Components v2 message flag (IS_COMPONENTS_V2 = 1 << 15).
const V2_FLAG: u64 = 1 << 15;

/// Maximum components per v2 message.
pub const MAX_V2_COMPONENTS: usize = 40;

/// Maximum characters in a single TextDisplay content.
pub const TEXT_DISPLAY_LIMIT: usize = 4000;

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

    let payload = serde_json::json!({
        "flags": V2_FLAG,
        "components": capped,
    });

    http.send_message(channel_id, attachments, &payload).await
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

/// Group a flat list of v2 components into message-sized chunks of at most
/// `MAX_V2_COMPONENTS` each.
pub fn group_into_v2_messages(components: Vec<serde_json::Value>) -> Vec<Vec<serde_json::Value>> {
    if components.len() <= MAX_V2_COMPONENTS {
        return vec![components];
    }
    components
        .chunks(MAX_V2_COMPONENTS)
        .map(|c| c.to_vec())
        .collect()
}
