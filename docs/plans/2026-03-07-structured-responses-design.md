# Structured Responses: Images, Attachments, and Citation Formatting

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Enable the GHOST to send images/files to the OPERATOR, format citations cleanly, and link messages to their sources.

**Architecture:** Skill + CLI command pattern for attachments (no new tools in model schema). Post-hoc citation extraction in the interface rendering layer. Two-phase message-to-source linking via a new `message_source` DB table with backfill during reflection.

**Tech Stack:** Rust (clap CLI, serenity Discord API, sqlx SQLite), Markdown skill files.

---

## Context

The GHOST previously had a `respond` structured output tool (removed in spec 24) that
forced all responses through a tool call with citations. It was removed because models
bypassed it, the interception logic was complex, and the citation format was brittle.

However, three capabilities are still needed:

1. **Sending images/files** — the image-generation skill saves to disk but the OPERATOR
   can't see the result
2. **Clean citation formatting** — the model writes verbose, inconsistent source blocks
3. **Message-to-source linking** — no record of which sources informed a message
4. **Future: interaction prompts** — yes/no questions, multi-choice pickers (deferred)

Industry review: no major system uses a monolithic "respond" tool. They all use plain text
responses combined with (a) post-hoc citation extraction, (b) dedicated tools for specific
interactions, and (c) API-level annotations.

## Design

### 1. Session identity for CLI commands

Add `GHOST_SESSION_ID` env var in `shell_command()` (`src/tools/shell.rs`), alongside the
existing `GHOST_CHANNEL_ID`. Child processes get session context from environment — the
model never needs to know its own session ID.

### 2. Image and attachment CLI commands

Two new CLI subcommands under `ghost`:

- `ghost send-image <path> [--caption "description"]` — send an image to the OPERATOR
- `ghost attach <path> [--caption "description"]` — send a generic file (CSV, JSON, etc.)

Both commands:

- Read `GHOST_CHANNEL_ID` and `GHOST_SESSION_ID` from environment
- Load config from the standard config path
- Send the file through the active interface (Discord for PoC, extensible later)
- Write a system message to the DB session for history (e.g. `[sent image: filename.png]`)
- Exit with success/failure message
- Only accept local file paths (use `curl`/`web_fetch` to download remote files first)

This is the simplest approach (no daemon coordination). The daemon doesn't need real-time
awareness because the model already knows what it did — it ran the shell command. The DB
record ensures the session history is complete.

### 3. Skill: `sending-attachments`

A lightweight skill that tells the model how to send files. Minimal context footprint
(~80 words). The model uses `run_shell_command` to call the CLI commands.

```markdown
---
name: sending-attachments
description:
  Use when you need to send an image, generated file, CSV, or any
  attachment to the OPERATOR.
---

# Sending Attachments

Send files to the OPERATOR. Session and channel are detected automatically
from environment — no IDs needed.

## Commands

**Image:**

    ghost send-image $WORKSPACE/tmp/my-image.png

**Image with caption:**

    ghost send-image $WORKSPACE/tmp/my-image.png --caption "Here's the chart"

**Any file:**

    ghost attach $WORKSPACE/tmp/data.csv

## When NOT to use

- Small data that fits in a message — just write it inline
- Code snippets — use markdown code blocks instead
```

### 4. Post-hoc citation formatting

**Prompt addition** (one line in `prompts/chat-system.md`):

> When citing sources, use [1], [2] inline. End with a Sources section listing
> [N] [Title](url).

**Post-processing** at the interface rendering layer. Each interface implements its own
citation formatting (Discord PoC: `src/interfaces/discord/markdown.rs`, future web UI
would have its own).

- After receiving the response text, detect a trailing "Sources" or "References" section
- Match URLs against recent `web_fetch` tool calls (web cache has page titles)
- Reformat as clean numbered markdown links: `[1] [Title](url)`
- Keep the DB message as-is (post-processing is display-only)

### 5. Message-to-source linking

New table to track which sources informed each message:

```sql
CREATE TABLE message_source (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    reference_id TEXT REFERENCES reference(id) ON DELETE SET NULL,
    url TEXT NOT NULL,
    title TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_message_source_message ON message_source(message_id);
CREATE INDEX idx_message_source_reference ON message_source(reference_id);
CREATE INDEX idx_message_source_url ON message_source(url);
```

**Two-phase linking:**

1. **On `on_end_turn`** (in `ChatHandler`): extract URLs from the response text using the
   existing `URL_RE` regex. Create `message_source` rows with `url` and `title` (parsed
   from the `[N] [Title](url)` format). `reference_id` is NULL at this point — the web
   cache hasn't been curated yet.

2. **During reflection/curation** (in `link_cited_edges` or alongside it): after
   `curate_references` moves cache files to `references/` and creates DB records, backfill
   `reference_id` on matching `message_source` rows:
   ```sql
   UPDATE message_source SET reference_id = ? WHERE url = ? AND reference_id IS NULL
   ```

This enables queries like:
- "What sources informed this message?" — `SELECT * FROM message_source WHERE message_id = ?`
- "Which messages cited this reference?" — `SELECT * FROM message_source WHERE reference_id = ?`

## Files

| File | Change |
|---|---|
| `src/tools/shell.rs` | Add `GHOST_SESSION_ID` env var to `shell_command()` |
| `src/cli/mod.rs` | Add `SendImage` / `Attach` subcommands |
| `src/cli/send.rs` (new) | CLI handler: read file, send via interface, write DB record |
| `src/main.rs` | Wire new CLI commands |
| `prompts/skills/sending-attachments/skill.md` (new) | Skill for sending files |
| `src/skills.rs` | Register the new default skill |
| `prompts/chat-system.md` | One-line citation format instruction |
| `src/interfaces/discord/markdown.rs` | Citation post-processing for display |
| `migrations/NNN_message_source.sql` (new) | `message_source` table |
| `src/db/knowledge/graph.rs` | Add `message_source` CRUD + backfill query |
| `src/chat/session.rs` | Extract URLs in `on_end_turn`, create `message_source` rows |
| `src/web/curation.rs` | Backfill `reference_id` during curation step |

## Non-goals

- **Daemon API / IPC**: Too much complexity for this feature. CLI commands send directly
  via the interface. The daemon API is a future concern (spec d: remote CLI).
- **Interaction prompts**: Deferred to a later spec. The skill + CLI pattern can be
  extended when needed.
- **Replacing model text output**: The model still responds with plain text. No structured
  output tool.
- **URL sending**: Use `curl` or `web_fetch` to download files first, then send local path.

---

## Implementation Plan

### Task 1: Add `GHOST_SESSION_ID` env var to shell tool

**Files:**
- Modify: `src/tools/shell.rs:20-45` (`shell_command` function)
- Modify: `src/tools/context.rs:12-21` (`ToolContext` — session_id already exists)

**Step 1: Update `shell_command` signature to accept session_id**

The function currently takes `channel_id: Option<&str>`. Add `session_id: Option<&str>`:

```rust
fn shell_command(
    command: &str,
    workspace: &std::path::Path,
    channel_id: Option<&str>,
    session_id: Option<&str>,
) -> tokio::process::Command {
    // ... existing nix/sh logic unchanged ...
    if let Some(id) = channel_id {
        cmd.env("GHOST_CHANNEL_ID", id);
    }
    if let Some(id) = session_id {
        cmd.env("GHOST_SESSION_ID", id);
    }
    cmd
}
```

**Step 2: Update both call sites in `execute()`**

In the `background` branch (~line 119):
```rust
let child = shell_command(&command_owned, &workspace_owned, channel_id.as_deref(), Some(&session_id))
```

In the foreground branch (~line 172):
```rust
let child = shell_command(command, &ctx.workspace, ctx.channel_id.as_deref(), Some(&ctx.session_id))
```

**Step 3: Update test helper**

In `test_ctx()` (~line 252), no changes needed — `session_id` is already `"test"` in
`ToolContext`. The `shell_command` calls in tests just gain the extra param.

**Step 4: Run tests**

Run: `cargo test --lib shell`
Expected: All existing shell tests pass.

**Step 5: Commit**

```
git add src/tools/shell.rs
git commit -m "feat: pass GHOST_SESSION_ID env var to shell child processes"
```

---

### Task 2: Migration — `message_source` table

**Files:**
- Create: `migrations/005_message_source.sql`

**Step 1: Write the migration**

```sql
CREATE TABLE message_source (
    id TEXT PRIMARY KEY NOT NULL,
    message_id TEXT NOT NULL REFERENCES message(id) ON DELETE CASCADE,
    reference_id TEXT REFERENCES reference(id) ON DELETE SET NULL,
    url TEXT NOT NULL,
    title TEXT,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_message_source_message ON message_source(message_id);
CREATE INDEX idx_message_source_reference ON message_source(reference_id);
CREATE INDEX idx_message_source_url ON message_source(url);
```

**Step 2: Verify migration applies**

Run: `cargo test --lib db::connection` (or any test that runs migrations)
Expected: PASS — migration applies cleanly.

**Step 3: Commit**

```
git add migrations/005_message_source.sql
git commit -m "feat: add message_source table for message-to-reference linking"
```

---

### Task 3: DB CRUD for `message_source`

**Files:**
- Modify: `src/db/knowledge/graph.rs` (add functions)
- Modify: `src/db/knowledge/mod.rs` (re-export new functions)

**Step 1: Add `create_message_source` function**

In `src/db/knowledge/graph.rs`, add:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(message_id = %message_id, url = %url))]
pub async fn create_message_source(
    db: &SqlitePool,
    message_id: &str,
    url: &str,
    title: Option<&str>,
) -> Result<String, DatabaseError> {
    let id = new_id();
    sqlx::query(
        "INSERT INTO message_source (id, message_id, url, title, created_at) \
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(message_id)
    .bind(url)
    .bind(title)
    .bind(now())
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "create_message_source",
        source,
    })?;
    Ok(id)
}
```

**Step 2: Add `backfill_message_source_references` function**

```rust
/// Backfill `reference_id` on message_source rows that match the given URL.
/// Called during reflection/curation after reference records are created.
#[tracing::instrument(skip_all, level = "debug", fields(url = %url, reference_id = %reference_id))]
pub async fn backfill_message_source_references(
    db: &SqlitePool,
    url: &str,
    reference_id: &str,
) -> Result<u64, DatabaseError> {
    let result = sqlx::query(
        "UPDATE message_source SET reference_id = ? \
         WHERE url = ? AND reference_id IS NULL",
    )
    .bind(reference_id)
    .bind(url)
    .execute(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message_source",
        operation: "backfill_message_source_references",
        source,
    })?;
    Ok(result.rows_affected())
}
```

**Step 3: Re-export from `src/db/knowledge/mod.rs`**

Add `create_message_source` and `backfill_message_source_references` to the pub use
re-exports (follow existing pattern in that file).

**Step 4: Run tests**

Run: `cargo check`
Expected: Compiles cleanly.

**Step 5: Commit**

```
git add src/db/knowledge/graph.rs src/db/knowledge/mod.rs
git commit -m "feat: add message_source CRUD and backfill queries"
```

---

### Task 4: Citation extraction in `on_end_turn`

**Files:**
- Modify: `src/chat/session.rs:456-482` (`ChatHandler::on_end_turn`)
- Create: `src/chat/citations.rs` (extraction logic)
- Modify: `src/chat/mod.rs` (add module)

**Step 1: Create `src/chat/citations.rs` with extraction logic**

```rust
use regex::Regex;
use std::sync::LazyLock;

/// A citation extracted from model response text.
#[derive(Debug, Clone)]
pub struct ExtractedCitation {
    pub url: String,
    pub title: Option<String>,
}

/// Match `[N] [Title](url)` or `[N] url` patterns in a Sources/References section.
static CITATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(\d+)\]\s+\[([^\]]+)\]\(([^)]+)\)|\[(\d+)\]\s+(https?://\S+)")
        .expect("citation regex")
});

/// Extract citations from the trailing Sources/References section of a response.
pub fn extract_citations(text: &str) -> Vec<ExtractedCitation> {
    // Find the Sources/References section
    let section_start = text
        .rfind("## Sources")
        .or_else(|| text.rfind("## References"))
        .or_else(|| text.rfind("**Sources**"))
        .or_else(|| text.rfind("Sources:"));

    let section = match section_start {
        Some(pos) => &text[pos..],
        None => return Vec::new(),
    };

    CITATION_RE
        .captures_iter(section)
        .map(|cap| {
            if let (Some(title), Some(url)) = (cap.get(2), cap.get(3)) {
                ExtractedCitation {
                    url: url.as_str().to_string(),
                    title: Some(title.as_str().to_string()),
                }
            } else if let Some(url) = cap.get(5) {
                ExtractedCitation {
                    url: url.as_str().trim_end_matches(|c: char| ".,;:)".contains(c)).to_string(),
                    title: None,
                }
            } else {
                ExtractedCitation {
                    url: String::new(),
                    title: None,
                }
            }
        })
        .filter(|c| !c.url.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_titled_citations() {
        let text = "Some answer text.\n\n\
            ## Sources\n\
            [1] [Tom's Hardware Review](https://tomshardware.com/reviews/test)\n\
            [2] [All3DP Guide](https://all3dp.com/guide)\n";

        let citations = extract_citations(text);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0].url, "https://tomshardware.com/reviews/test");
        assert_eq!(citations[0].title.as_deref(), Some("Tom's Hardware Review"));
        assert_eq!(citations[1].url, "https://all3dp.com/guide");
    }

    #[test]
    fn extract_bare_url_citations() {
        let text = "Answer.\n\nSources:\n[1] https://example.com/page\n";

        let citations = extract_citations(text);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].url, "https://example.com/page");
        assert!(citations[0].title.is_none());
    }

    #[test]
    fn no_sources_section() {
        let text = "Just an answer with no sources.";
        assert!(extract_citations(text).is_empty());
    }
}
```

**Step 2: Add module to `src/chat/mod.rs`**

Add `pub(crate) mod citations;` to the module declarations.

**Step 3: Run tests**

Run: `cargo test --lib chat::citations`
Expected: All 3 tests pass.

**Step 4: Wire into `ChatHandler::on_end_turn`**

In `src/chat/session.rs`, after the `create_message_with_metadata` call in
`ChatHandler::on_end_turn` (~line 463-472), add citation extraction:

```rust
async fn on_end_turn(
    &mut self,
    message: String,
    stop_reason: StopReason,
    tool_uses: &[Value],
    raw_output: Option<Vec<Value>>,
) -> Result<ChatResult, ChatError> {
    let msg_id = db::sessions::create_message_with_metadata(
        self.session_chat.db(),
        self.session_thing,
        "assistant",
        &message,
        Some(tool_uses.to_vec()),
        None,
        raw_output,
    )
    .await?;

    // Extract citations and create message_source records
    let citations = super::citations::extract_citations(&message);
    for citation in &citations {
        let _ = db::knowledge::create_message_source(
            self.session_chat.db(),
            &msg_id,
            &citation.url,
            citation.title.as_deref(),
        )
        .await;
    }

    Ok(ChatResult {
        message,
        stop_reason: if stop_reason == StopReason::MaxTokens {
            ChatStopReason::MaxTokens
        } else {
            ChatStopReason::EndTurn
        },
    })
}
```

**Note:** `create_message_with_metadata` currently returns `Result<(), ChatError>`. It
needs to be updated to return the message ID. Check the function signature in
`src/db/sessions.rs` — if it doesn't return the ID, update it to do so (it already
generates `new_id()` internally, just needs to return it).

**Step 5: Run tests**

Run: `cargo test --lib chat`
Expected: PASS.

**Step 6: Commit**

```
git add src/chat/citations.rs src/chat/mod.rs src/chat/session.rs src/db/sessions.rs
git commit -m "feat: extract citations from responses and create message_source records"
```

---

### Task 5: Backfill `reference_id` during curation

**Files:**
- Modify: `src/web/curation.rs:226-338` (`link_cited_edges` function)

**Step 1: Add backfill call inside the `link_cited_edges` loop**

After a reference record is found or created (the `ref_record` binding, ~line 305), add:

```rust
// Backfill message_source rows that cited this URL
if let Err(e) = db::knowledge::backfill_message_source_references(
    db,
    &file.url,
    &ref_record.id,
)
.await
{
    logfire::warn!(
        "link_cited_edges: failed to backfill message_source",
        url = file.url.clone(),
        error = e.to_string(),
    );
}
```

Insert this right after `ref_record` is resolved (before the note-matching loop at
~line 308).

**Step 2: Run tests**

Run: `cargo test --lib web::curation`
Expected: PASS (existing tests don't touch message_source).

**Step 3: Commit**

```
git add src/web/curation.rs
git commit -m "feat: backfill message_source.reference_id during curation"
```

---

### Task 6: Update system prompt for citation format

**Files:**
- Modify: `prompts/chat-system.md:57-67` (Sources and Citations section)

**Step 1: Replace the Sources and Citations section**

Replace the current block (~lines 57-67):

```markdown
## Sources and Citations

> [!IMPORTANT] When using web sources in your response, always include the URL so the
> OPERATOR can verify the information. Never reply without citing adequate sources.

When citing sources, use numbered references [1], [2] inline in your text. End your
response with a Sources section:

```
## Sources
[1] [Page Title](https://url)
[2] [Page Title](https://url)
```

- For notes: mention the file path (e.g., `notes/rust-patterns.md`)
- For references: mention the file path (e.g., `references/rust-patterns/ownership.md`)
- For web fetches: use the original URL from the cached page
- For web searches: use the result URL directly
```

**Step 2: Commit**

```
git add prompts/chat-system.md
git commit -m "feat: add citation format instructions to system prompt"
```

---

### Task 7: CLI `send-image` and `attach` commands

**Files:**
- Create: `src/cli/send.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create `src/cli/send.rs`**

```rust
use std::path::PathBuf;

use clap::Subcommand;
use serenity::builder::{CreateAttachment, CreateMessage};
use serenity::http::Http;
use serenity::model::id::ChannelId;

use crate::config;
use crate::db;
use crate::error::GhostError;

#[derive(Debug, Subcommand)]
pub enum SendCommand {
    /// Send an image to the OPERATOR
    Image {
        /// Path to the image file
        path: PathBuf,
        /// Optional caption
        #[arg(long)]
        caption: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum AttachCommand {
    // attach is not a subcommand group — handle it as a direct command
}

/// Send a file (image or generic) to the OPERATOR via Discord.
async fn send_file(
    path: &std::path::Path,
    caption: Option<&str>,
    is_image: bool,
) -> Result<(), GhostError> {
    let channel_id: u64 = std::env::var("GHOST_CHANNEL_ID")
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "GHOST_CHANNEL_ID not set — this command must be run from a GHOST shell session",
            )
        })?
        .parse()
        .map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "GHOST_CHANNEL_ID is not a valid u64",
            )
        })?;

    let session_id = std::env::var("GHOST_SESSION_ID").ok();

    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", path.display()),
        )
        .into());
    }

    let file_data = std::fs::read(path)?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file");

    // Send via Discord
    let token =
        std::env::var("DISCORD_BOT_TOKEN").map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "DISCORD_BOT_TOKEN not set")
        })?;

    let http = Http::new(&token);
    let channel = ChannelId::new(channel_id);

    let attachment = CreateAttachment::bytes(file_data, filename);
    let mut message = CreateMessage::new().add_file(attachment);
    if let Some(cap) = caption {
        message = message.content(cap);
    }

    channel
        .send_message(&http, message)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    // Record in session history
    if let Some(ref sid) = session_id {
        let config = config::load()?;
        crate::config_workspace::bootstrap_workspace(&config)?;
        let db_pool =
            db::connect(&config.workspace, config.embeddings.dimension).await?;
        let kind = if is_image { "image" } else { "file" };
        let cap_suffix = caption
            .map(|c| format!(" — {c}"))
            .unwrap_or_default();
        let msg = format!("[sent {kind}: {filename}{cap_suffix}]");
        db::sessions::create_message(&db_pool, sid, "system", &msg).await?;
    }

    let kind = if is_image { "Image" } else { "File" };
    println!("{kind} sent: {filename}");
    Ok(())
}

pub async fn execute_send_image(path: PathBuf, caption: Option<String>) -> Result<(), GhostError> {
    send_file(&path, caption.as_deref(), true).await
}

pub async fn execute_attach(path: PathBuf, caption: Option<String>) -> Result<(), GhostError> {
    send_file(&path, caption.as_deref(), false).await
}
```

**Step 2: Add module to `src/cli/mod.rs`**

Add `pub mod send;` to the module declarations.

**Step 3: Wire into `src/main.rs`**

Add two new variants to `Commands`:

```rust
/// Send an image to the OPERATOR
SendImage {
    /// Path to the image file
    path: std::path::PathBuf,
    /// Optional caption
    #[arg(long)]
    caption: Option<String>,
},
/// Send a file attachment to the OPERATOR
Attach {
    /// Path to the file
    path: std::path::PathBuf,
    /// Optional caption
    #[arg(long)]
    caption: Option<String>,
},
```

And in `dispatch()`:

```rust
Commands::SendImage { path, caption } => ghost::cli::send::execute_send_image(path, caption).await,
Commands::Attach { path, caption } => ghost::cli::send::execute_attach(path, caption).await,
```

**Step 4: Verify compilation**

Run: `cargo check`
Expected: Compiles.

**Step 5: Manual smoke test**

```sh
GHOST_CHANNEL_ID=123 DISCORD_BOT_TOKEN=fake cargo run -- send-image /tmp/test.png
```
Expected: Error about invalid token (not a crash).

**Step 6: Commit**

```
git add src/cli/send.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add ghost send-image and ghost attach CLI commands"
```

---

### Task 8: Register `sending-attachments` skill

**Files:**
- Create: `prompts/skills/sending-attachments/skill.md`
- Modify: `src/skills.rs`

**Step 1: Create the skill file**

Create `prompts/skills/sending-attachments/skill.md` with the content from the design
doc section 3.

**Step 2: Register in `src/skills.rs`**

Add to `DEFAULT_SKILLS` array (alphabetical order, after `reference-import`):

```rust
DefaultSkill {
    path: "sending-attachments",
    files: &[(
        "skill.md",
        include_str!("../prompts/skills/sending-attachments/skill.md"),
    )],
},
```

**Step 3: Update skill count assertion**

In the test `install_default_skills_creates_files`, update:
```rust
assert_eq!(DEFAULT_SKILLS.len(), 23);
```

**Step 4: Run tests**

Run: `cargo test --lib skills`
Expected: All skill tests pass.

**Step 5: Commit**

```
git add prompts/skills/sending-attachments/skill.md src/skills.rs
git commit -m "feat: add sending-attachments skill"
```

---

### Task 9: Citation post-processing for Discord

**Files:**
- Modify: `src/interfaces/discord/markdown.rs`

This is the most nuanced task. The goal: detect a trailing Sources/References section in
the model's response and ensure it renders cleanly in Discord (compact numbered links,
no verbose descriptions per source).

**Step 1: Add a citation-cleaning pass**

Add a function that runs before `markdown_to_v2_components`:

```rust
/// Clean up a trailing Sources/References section for compact display.
///
/// Detects `## Sources`, `## References`, `**Sources**`, or `Sources:` sections
/// at the end of the text. Strips verbose per-source descriptions, keeping only
/// `[N] [Title](url)` lines. Passes through already-clean sections unchanged.
pub fn clean_citation_section(text: &str) -> String {
    // Find the sources section
    let section_markers = ["## Sources", "## References", "**Sources**", "Sources:"];
    let section_start = section_markers
        .iter()
        .filter_map(|marker| text.rfind(marker))
        .max();

    let Some(start) = section_start else {
        return text.to_string();
    };

    let before = &text[..start];
    let section = &text[start..];

    // Extract URLs from the section, reformat as clean links
    let url_re = regex::Regex::new(r"https?://[^\s\]\)>,]+").unwrap();
    let titled_re =
        regex::Regex::new(r"\[(\d+)\]\s+\[([^\]]+)\]\(([^)]+)\)").unwrap();

    // If already in clean [N] [Title](url) format, pass through
    if titled_re.is_match(section) {
        return text.to_string();
    }

    // Otherwise, extract URLs and reformat
    let urls: Vec<&str> = url_re
        .find_iter(section)
        .map(|m| m.as_str().trim_end_matches(|c: char| ".,;:)".contains(c)))
        .collect();

    if urls.is_empty() {
        return text.to_string();
    }

    let mut clean = before.trim_end().to_string();
    clean.push_str("\n\n## Sources\n");
    for (i, url) in urls.iter().enumerate() {
        clean.push_str(&format!("[{}] {}\n", i + 1, url));
    }

    clean
}
```

**Step 2: Wire into `markdown_to_v2_components`**

At the top of `markdown_to_v2_components`, add:

```rust
pub fn markdown_to_v2_components(text: &str) -> MarkdownComponents {
    let text = &clean_citation_section(text);
    // ... rest unchanged
```

**Step 3: Add tests**

```rust
#[test]
fn clean_citation_section_passes_through_clean() {
    let text = "Answer.\n\n## Sources\n[1] [Title](https://example.com)\n";
    assert_eq!(clean_citation_section(text), text);
}

#[test]
fn clean_citation_section_reformats_verbose() {
    let text = "Answer.\n\n## Sources\n\
        1. https://example.com/page - This is a really long description\n\
        2. https://other.com/article - Another verbose description\n";
    let cleaned = clean_citation_section(text);
    assert!(cleaned.contains("[1] https://example.com/page"));
    assert!(cleaned.contains("[2] https://other.com/article"));
    assert!(!cleaned.contains("really long description"));
}

#[test]
fn clean_citation_section_no_sources() {
    let text = "Just an answer.";
    assert_eq!(clean_citation_section(text), text);
}
```

**Step 4: Run tests**

Run: `cargo test --lib discord::markdown`
Expected: All tests pass.

**Step 5: Commit**

```
git add src/interfaces/discord/markdown.rs
git commit -m "feat: clean citation sections for compact Discord display"
```

---

### Task 10: Final integration — `just ci`

**Step 1: Run full CI**

Run: `just ci`
Expected: Format, check, clippy, and all tests pass.

**Step 2: Fix any issues**

Address clippy warnings, format issues, or test failures.

**Step 3: Final commit (if any fixes needed)**

```
git commit -m "chore: fix CI issues from structured-responses feature"
```
