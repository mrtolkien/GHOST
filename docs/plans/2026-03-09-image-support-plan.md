# Image Support Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Enable GHOST to see images — both from Discord chat uploads and via the
`read_file` tool reading image files from disk.

**Architecture:** Add `ContentBlock::Image` variant and `ToolOutput` type. Images stored
as workspace-relative paths in DB, base64-encoded at provider send time with
resize/compression (max 1568px, JPEG q85). Discord classifies attachments by MIME type.
Tool return type changes from `String` to `ToolOutput` (text + optional images).

**Tech Stack:** `image` crate (resize/compress), `base64` (already dep), existing
provider infrastructure.

**Reference code:** `~/Development/t-koma` — predecessor had full image support. Key
files: `t-koma-gateway/src/chat/history.rs` (load_image_base64, compress_image),
`t-koma-gateway/src/discord/bot.rs` (attachment classification).

---

### Task 1: Add `image` crate dependency

**Files:**

- Modify: `Cargo.toml`

**Step 1: Add dependency**

Add to `[dependencies]`:

```toml
image = { version = "0.25", default-features = false, features = ["jpeg", "png", "gif", "webp"] }
```

**Step 2: Verify it compiles**

Run: `cargo check` Expected: PASS

**Step 3: Commit**

```
feat: add image crate dependency for vision support
```

---

### Task 2: Create image utility module

**Files:**

- Create: `src/images.rs`
- Modify: `src/main.rs` (add `mod images;`)

**Step 1: Write tests for image utilities**

Test `is_image_extension` for known image types and non-image types. Test
`mime_type_from_extension` returns correct MIME types. Test `compress_image` with a
small synthetic image (create a 2x2 PNG in memory using the `image` crate).

**Step 2: Run tests to verify they fail**

Run: `cargo test images::tests` Expected: FAIL (module doesn't exist)

**Step 3: Implement the module**

Port from t-koma (`t-koma-gateway/src/chat/history.rs:245-302`):

```rust
// src/images.rs

use std::path::Path;
use base64::Engine;

const MAX_IMAGE_DIMENSION: u32 = 1568;
const JPEG_QUALITY: u8 = 85;

const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp"];

pub fn is_image_extension(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

pub fn mime_type_from_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

/// Load an image from disk, optionally resize/compress, return (base64, mime_type).
pub fn load_image_base64(path: &Path) -> Result<(String, String), String> {
    let bytes = std::fs::read(path)
        .map_err(|e| format!("failed to read image '{}': {e}", path.display()))?;

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let original_mime = mime_type_from_extension(ext).to_string();

    if let Some((compressed, mime)) = compress_image(&bytes) {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
        Ok((b64, mime))
    } else {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok((b64, original_mime))
    }
}

/// Resize if > MAX_IMAGE_DIMENSION, recompress as JPEG.
/// Returns None if image is already small enough.
fn compress_image(bytes: &[u8]) -> Option<(Vec<u8>, String)> {
    let img = image::load_from_memory(bytes).ok()?;
    let (w, h) = (img.width(), img.height());
    if w <= MAX_IMAGE_DIMENSION && h <= MAX_IMAGE_DIMENSION {
        return None;
    }
    let resized = img.resize(MAX_IMAGE_DIMENSION, MAX_IMAGE_DIMENSION,
                             image::imageops::FilterType::Lanczos3);
    let mut buf = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut buf, image::ImageFormat::Jpeg).ok()?;
    Some((buf.into_inner(), "image/jpeg".to_string()))
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test images::tests` Expected: PASS

**Step 5: Commit**

```
feat: add image utility module (load, compress, mime detection)
```

---

### Task 3: Add `ContentBlock::Image` variant

**Files:**

- Modify: `src/providers/types.rs`
- Modify: `src/chat/convert.rs`
- Modify: `src/chat/compaction.rs`

**Step 1: Add Image variant to ContentBlock**

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    Image { path: String, mime_type: String, filename: String },  // NEW
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    RawOutput { original_type: String, value: Value },
}
```

**Step 2: Handle Image in all existing match arms**

Find every `match block { ContentBlock::... }` pattern and add
`ContentBlock::Image { .. }`. Key locations:

- `src/chat/convert.rs`: `extract_text_content` — skip Image blocks
- `src/chat/convert.rs`: `extract_tool_use_blocks` — skip Image blocks
- `src/chat/convert.rs`: `raw_output_to_values` — skip Image blocks
- `src/chat/convert.rs`: `tool_results_to_values` — skip Image blocks
- `src/chat/compaction.rs`: all ContentBlock matches — pass through Image blocks

**Step 3: Verify it compiles and tests pass**

Run: `just ci` Expected: PASS (no logic changes, just exhaustiveness)

**Step 4: Commit**

```
feat: add ContentBlock::Image variant
```

---

### Task 4: Add `ToolOutput` type and update `Tool` trait

**Files:**

- Create: `src/tools/output.rs`
- Modify: `src/tools/mod.rs`
- Modify: `src/tools/manager.rs` (Tool trait + ToolManager::execute)
- Modify: `src/chat/session.rs` (execute_single_tool)
- Modify: all tool implementations (return ToolOutput)

**Step 1: Create ToolOutput type**

```rust
// src/tools/output.rs

/// Result of a tool execution. Most tools return text-only output.
/// Tools like `read_file` can include images alongside text.
pub struct ToolOutput {
    pub text: String,
    pub images: Vec<ImageRef>,
}

pub struct ImageRef {
    pub path: String,
    pub mime_type: String,
    pub filename: String,
}

impl ToolOutput {
    /// Create a text-only output (most tools).
    pub fn text(text: String) -> Self {
        Self { text, images: vec![] }
    }
}

impl From<String> for ToolOutput {
    fn from(text: String) -> Self {
        Self::text(text)
    }
}
```

**Step 2: Update Tool trait**

Change `Tool::execute` return type from `Result<String, ToolError>` to
`Result<ToolOutput, ToolError>`.

**Step 3: Update ToolManager::execute**

Change return type, update logging (log `output.text` instead of `output`).

**Step 4: Update execute_single_tool in session.rs**

```rust
async fn execute_single_tool(...) -> Vec<ContentBlock> {
    // ... existing ctx setup ...
    match self.tool_manager.execute(name, input, &tool_ctx).await {
        Ok(output) => {
            let mut blocks = vec![ContentBlock::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: output.text,
                is_error: false,
            }];
            for img in output.images {
                blocks.push(ContentBlock::Image {
                    path: img.path,
                    mime_type: img.mime_type,
                    filename: img.filename,
                });
            }
            blocks
        }
        Err(error) => vec![ContentBlock::ToolResult {
            tool_use_id: tool_use_id.to_string(),
            content: render_tool_error(error),
            is_error: true,
        }],
    }
}
```

Change `execute_tool_calls` to flatten: `futures.await.into_iter().flatten().collect()`.

**Step 5: Update all tool implementations**

Every tool's `execute` method: change return type to `Result<ToolOutput, ToolError>`,
wrap existing `Ok(string)` returns in `Ok(ToolOutput::text(string))`.

Affected files (wrap Ok(string) → Ok(ToolOutput::text(string)) mechanically):

- `src/tools/shell.rs`
- `src/tools/read_file.rs`
- `src/tools/write_file.rs`
- `src/tools/file_edit.rs`
- `src/tools/knowledge_search.rs`
- `src/tools/web_search.rs`
- `src/tools/web_fetch.rs`
- `src/tools/agent_control.rs`
- `src/tools/todo.rs`
- `src/tools/note_write.rs`
- Any Lua custom tools (check `src/scripting/`)

**Step 6: Run tests**

Run: `just ci` Expected: PASS

**Step 7: Commit**

```
refactor: change Tool::execute return type to ToolOutput
```

---

### Task 5: Image support in `read_file`

**Files:**

- Modify: `src/tools/read_file.rs`

**Step 1: Write test for image file reading**

```rust
#[tokio::test]
async fn read_image_file_returns_image_output() {
    let workspace = TempDir::new().unwrap();
    // Create a minimal 1x1 PNG
    let img = image::RgbImage::new(1, 1);
    let path = workspace.path().join("test.png");
    img.save(&path).unwrap();

    let ctx = test_ctx_in(workspace.path());
    let output = ReadFile.execute(json!({"path": "test.png"}), &ctx).await.unwrap();

    assert_eq!(output.images.len(), 1);
    assert_eq!(output.images[0].mime_type, "image/png");
    assert_eq!(output.images[0].filename, "test.png");
    assert!(output.text.contains("test.png"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test read_file::tests::read_image_file` Expected: FAIL

**Step 3: Implement image detection in read_file**

At the top of `ReadFile::execute`, after resolving the path, check the extension:

```rust
let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
if crate::images::is_image_extension(ext) {
    let mime = crate::images::mime_type_from_extension(ext).to_string();
    let filename = path.file_name()
        .and_then(|f| f.to_str())
        .unwrap_or(raw_path)
        .to_string();
    return Ok(ToolOutput {
        text: format!("Image file: {raw_path}"),
        images: vec![ImageRef {
            path: path.to_string_lossy().to_string(),
            mime_type: mime,
            filename,
        }],
    });
}
```

**Step 4: Run tests**

Run: `cargo test read_file::tests` Expected: PASS

**Step 5: Update tool schema description**

Update the description to mention image support:

```
"Read a file from the workspace with line numbers. Image files (PNG, JPEG, GIF, WebP) are returned as viewable images."
```

**Step 6: Commit**

```
feat: read_file returns images for image files
```

---

### Task 6: Provider image serialization — OpenAI compatible

**Files:**

- Modify: `src/providers/openai_compatible.rs`

**Step 1: Write test for image content in user message**

```rust
#[test]
fn convert_messages_includes_image_as_content_array() {
    let request = ChatRequest {
        model: "test".to_string(),
        messages: vec![ChatMessage {
            role: Role::User,
            content: vec![
                ContentBlock::Text { text: "Look at this".to_string() },
                ContentBlock::Image {
                    path: "/tmp/test.png".to_string(),
                    mime_type: "image/png".to_string(),
                    filename: "test.png".to_string(),
                },
            ],
        }],
        // ... defaults ...
    };
    let messages = convert_messages(&request);
    let content = messages[0].content.as_ref().unwrap();
    assert!(content.is_array());
    let parts = content.as_array().unwrap();
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "image_url");
}
```

**Step 2: Run test to verify it fails**

Expected: FAIL (Image blocks are currently skipped)

**Step 3: Implement image handling in convert_messages**

In the `convert_messages` function, track image parts alongside text parts. When any
images exist, emit a content array instead of a plain string:

```rust
// Inside the for block loop:
ContentBlock::Image { path, mime_type, .. } => {
    match crate::images::load_image_base64(std::path::Path::new(path)) {
        Ok((b64, mime)) => {
            image_parts.push(json!({
                "type": "image_url",
                "image_url": {
                    "url": format!("data:{mime};base64,{b64}")
                }
            }));
        }
        Err(e) => {
            text_parts.push(format!("[Image unavailable: {e}]"));
        }
    }
}
```

When building the content value: if `image_parts` is non-empty, build an array of
`[{"type": "text", "text": joined_text}, ...image_parts]`. Otherwise keep the current
`Value::String` behavior.

Also handle Image blocks in tool result messages — when a ToolResult is followed by
Image blocks, include the images in the tool message content array.

**Step 4: Run tests**

Run: `cargo test openai_compatible::tests` Expected: PASS

**Step 5: Commit**

```
feat: OpenAI-compatible provider image support
```

---

### Task 7: Provider image serialization — Codex Responses API

**Files:**

- Modify: `src/providers/codex_responses.rs`

**Step 1: Write test**

Test that `build_codex_request_body` converts Image blocks in user messages to
`input_image` content parts.

**Step 2: Implement**

In `build_codex_request_body`, when processing message content blocks:

```rust
ContentBlock::Image { path, .. } => {
    match crate::images::load_image_base64(std::path::Path::new(path)) {
        Ok((b64, mime)) => {
            // Codex Responses API uses input_image with data URI
            image_parts.push(json!({
                "type": "input_image",
                "image_url": format!("data:{mime};base64,{b64}")
            }));
        }
        Err(_) => {} // Skip unavailable images
    }
}
```

The `CodexInputPart` enum needs extending to support image parts. Either:

- Add a new variant: `InputImage { image_url: String }`
- Or use `serde_json::Value` for the content array

Since `CodexInputPart` is currently `{ type, text }`, change `content` in the Message
variant to `Vec<Value>` to support heterogeneous parts, OR add a separate
`CodexImagePart` type.

For tool results (Image blocks after ToolResult): emit a separate user message with the
image after the `FunctionCallOutput` item.

**Step 3: Run tests**

Run: `cargo test codex_responses::tests` Expected: PASS

**Step 4: Commit**

```
feat: Codex Responses API image support
```

---

### Task 8: DB storage for image content blocks

**Files:**

- Modify: `src/db/sessions.rs` (`create_message`, `create_message_with_metadata`)
- Modify: `src/chat/convert.rs` (`convert_stored_message_to_provider_message`)
- Modify: `src/chat/session.rs` (`chat()` signature)

**Step 1: Add `images` column to message table**

New migration adding `images TEXT` (JSON) column to `message` table. Stores array of
`{"path": "...", "mime_type": "...", "filename": "..."}` objects.

This is simpler than converting `content` to a full JSON array — it's additive and
doesn't change how existing text content works.

**Step 2: Update create_message_with_metadata**

Add `images: Option<Vec<Value>>` parameter. Serialize to JSON TEXT.

**Step 3: Update MessageRecord**

Add `images: Option<String>` field + `images_parsed()` helper.

**Step 4: Update convert_stored_message_to_provider_message**

After pushing the Text block, parse images and push `ContentBlock::Image` blocks.

**Step 5: Update chat() to accept content blocks**

Change `SessionChat::chat` to accept `Vec<ContentBlock>` instead of `&str` for user
message content. Store text + images separately in the DB call.

**Step 6: Run tests**

Run: `just ci` Expected: PASS

**Step 7: Commit**

```
feat: store and load image content blocks in DB
```

---

### Task 9: Discord image intake

**Files:**

- Modify: `src/interfaces/discord/bot.rs` (`process_attachments`, `handle_message`)

**Step 1: Change process_attachments return type**

Return `Vec<ContentBlock>` instead of `String`. Classify attachments by MIME type:

```rust
const IMAGE_MIME_TYPES: &[&str] = &[
    "image/jpeg", "image/png", "image/gif", "image/webp",
];

fn is_image_mime(mime: &str) -> bool {
    IMAGE_MIME_TYPES.contains(&mime)
}
```

For image attachments: return `ContentBlock::Image { path, mime_type, filename }`. For
other files: return `ContentBlock::Text { text: "[File uploaded: ...]" }`.

Detect MIME type from Discord's `content_type` field, falling back to extension.

**Step 2: Update handle_message**

Instead of joining attachment text with message text, build a `Vec<ContentBlock>`:

- `ContentBlock::Text { text: user_message }` (if non-empty)
- Image/text blocks from `process_attachments`

Pass this `Vec<ContentBlock>` to `session_chat.chat()`.

**Step 3: Run tests**

Run: `just ci` Expected: PASS

**Step 4: Commit**

```
feat: Discord image attachments passed as image content blocks
```

---

### Task 10: Update compaction for images

**Files:**

- Modify: `src/chat/compaction.rs`

**Step 1: Handle Image blocks in compaction**

In Phase 1 (tool result masking), when masking old tool results, replace Image blocks
with a text placeholder: `[image: filename]`. This avoids sending stale base64 data in
compacted history.

In token estimation, count Image blocks as ~1000 tokens (rough estimate for a compressed
image).

**Step 2: Run tests**

Run: `cargo test compaction::tests` Expected: PASS

**Step 3: Commit**

```
feat: handle Image blocks in compaction (mask old images)
```

---

### Task 11: Integration verification

**Step 1: Run full CI**

Run: `just ci` Expected: PASS

**Step 2: Manual smoke test**

1. Start GHOST, send a message with an image attachment via Discord
2. Verify the model acknowledges seeing the image content
3. Ask GHOST to read an image file with `read_file`
4. Verify the model describes the image

**Step 3: Final commit (if any fixups needed)**

---

## Notes

- **No migration needed** — pre-alpha, workspace can be recreated
- **Image paths are absolute on disk** — base64 encoding happens at provider send time,
  not at storage time. This avoids storing large blobs in SQLite.
- **Compression is defensive** — only triggers for images > 1568px. Most Discord uploads
  and workspace images are already reasonable size.
- **Graceful degradation** — if image loading fails (deleted file, corrupt), providers
  insert `[Image unavailable: error]` text placeholder instead of failing the request.
