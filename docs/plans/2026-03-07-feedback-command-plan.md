# `/feedback` Command — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this
> plan task-by-task.

**Goal:** Discord `/feedback` slash command that snapshots session context (DB +
transcript + description) to a workspace folder for offline triage.

**Architecture:** New slash command in Discord bot, a `save_feedback` module under
`src/interfaces/discord/`, a new DB query for last N messages, and a Claude Code skill
for consuming feedback folders.

**Tech Stack:** serenity (slash command), tokio::fs (file ops), chrono (timestamp), sqlx
(query), existing types.

---

### Task 1: Add `get_last_n_messages` DB query

**Files:**

- Modify: `src/db/sessions.rs`

**Step 1: Add the query function**

After the existing `get_last_message` function (~line 325), add:

```rust
#[tracing::instrument(skip_all, level = "debug", fields(session_id = %session_id, n = n))]
pub async fn get_last_n_messages(
    db: &SqlitePool,
    session_id: &str,
    n: usize,
) -> Result<Vec<MessageRecord>, DatabaseError> {
    // Subquery to get last N in DESC order, then wrap to re-sort ASC.
    sqlx::query_as::<_, MessageRecord>(
        "SELECT * FROM (
             SELECT * FROM message WHERE session_id = ? ORDER BY created_at DESC LIMIT ?
         ) ORDER BY created_at ASC",
    )
    .bind(session_id)
    .bind(n as i64)
    .fetch_all(db)
    .await
    .map_err(|source| DatabaseError::Query {
        table: "message",
        operation: "get_last_n",
        source,
    })
}
```

**Step 2: Run `just ci`**

**Step 3: Commit**

```
feat: add get_last_n_messages DB query
```

---

### Task 2: Create `feedback` module with `save_feedback` + `make_slug` + transcript rendering

**Files:**

- Create: `src/interfaces/discord/feedback.rs`
- Modify: `src/interfaces/discord/mod.rs` (add `mod feedback;`)

**Step 1: Write `src/interfaces/discord/feedback.rs`**

````rust
use std::path::Path;

use sqlx::SqlitePool;
use tracing::warn;

use crate::db;
use crate::db::sessions::MessageRecord;

/// Max characters for tool call arguments / tool result output in transcript.
const TRUNCATE_LEN: usize = 2000;

/// Slugify the first few words of a feedback message for folder naming.
pub fn make_slug(message: &str) -> String {
    message
        .split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '-')
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Save a feedback snapshot: feedback.md, transcript.md, ghost.db copy.
pub async fn save_feedback(
    workspace: &Path,
    feedback_dir: &Path,
    db: &SqlitePool,
    session_id: &str,
    message: &str,
) -> Result<String, std::io::Error> {
    tokio::fs::create_dir_all(feedback_dir).await?;

    // feedback.md
    let timestamp = chrono::Utc::now().to_rfc3339();
    let feedback_md = format!(
        "# Feedback\n\n\
         **Timestamp:** {timestamp}\n\
         **Session ID:** {session_id}\n\n\
         ## Issue\n\n\
         {message}\n"
    );
    tokio::fs::write(feedback_dir.join("feedback.md"), &feedback_md).await?;

    // transcript.md
    let transcript = match render_transcript(db, session_id).await {
        Ok(t) => t,
        Err(e) => {
            warn!("Failed to render transcript: {e}");
            format!("(failed to render transcript: {e})")
        }
    };
    tokio::fs::write(feedback_dir.join("transcript.md"), &transcript).await?;

    // ghost.db copy
    let db_src = workspace.join("ghost.db");
    if db_src.exists() {
        tokio::fs::copy(&db_src, feedback_dir.join("ghost.db")).await?;
    }

    // Return the folder name for the ephemeral reply
    let folder_name = feedback_dir
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    Ok(folder_name)
}

async fn render_transcript(
    db: &SqlitePool,
    session_id: &str,
) -> Result<String, crate::db::DatabaseError> {
    let messages = db::sessions::get_last_n_messages(db, session_id, 10).await?;
    let mut out = String::from("# Session Transcript\n\n");
    out.push_str(&format!("Session: {session_id}\n"));
    out.push_str(&format!("Messages: {} (last 10)\n\n", messages.len()));

    for msg in &messages {
        render_message(&mut out, msg);
    }

    Ok(out)
}

fn render_message(out: &mut String, msg: &MessageRecord) {
    out.push_str(&format!(
        "---\n\n### {} — {}\n\n",
        msg.role, msg.created_at
    ));

    // Content
    if !msg.content.is_empty() {
        out.push_str(&msg.content);
        out.push_str("\n\n");
    }

    // Tool calls
    if let Some(calls) = msg.tool_calls_parsed() {
        for call in calls {
            let name = call
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| call.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let args = call
                .get("function")
                .and_then(|f| f.get("arguments"))
                .or_else(|| call.get("input"))
                .map(|v| truncate(&format!("{v}"), TRUNCATE_LEN))
                .unwrap_or_default();
            out.push_str(&format!("**Tool call:** `{name}`\n```\n{args}\n```\n\n"));
        }
    }

    // Tool results
    if let Some(results) = msg.tool_results_parsed() {
        for res in results {
            let output = res
                .get("output")
                .or_else(|| res.get("content"))
                .map(|v| truncate(&format!("{v}"), TRUNCATE_LEN))
                .unwrap_or_default();
            out.push_str(&format!("**Tool result:**\n```\n{output}\n```\n\n"));
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…(truncated)", &s[..max])
    }
}
````

**Step 2: Add `mod feedback;` to `src/interfaces/discord/mod.rs`**

**Step 3: Run `just ci`**

**Step 4: Commit**

```
feat: feedback module with save_feedback and transcript rendering
```

---

### Task 3: Register and handle `/feedback` slash command

**Files:**

- Modify: `src/interfaces/discord/bot.rs`

**Step 1: Add import at top**

```rust
use super::feedback;
```

**Step 2: Register the command in `ready()` (~line 643)**

Add to the `commands` vec:

```rust
CreateCommand::new("feedback")
    .description("Report an issue with the last interaction")
    .add_option(
        serenity::builder::CreateCommandOption::new(
            serenity::model::application::CommandOptionType::String,
            "message",
            "What went wrong?",
        )
        .required(true),
    ),
```

**Step 3: Add the match arm in `interaction_create()` (~line 631, before `_ => {}`)**

```rust
"feedback" => {
    let feedback_message = command
        .data
        .options
        .iter()
        .find(|o| o.name == "message")
        .and_then(|o| o.value.as_str())
        .unwrap_or("(no message)")
        .to_string();

    let session_id = match self.resolve_session(channel_id).await {
        Ok(id) => id,
        Err(e) => {
            error!("Failed to resolve session for /feedback: {e}");
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Failed to resolve session.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }
    };

    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H-%M-%S").to_string();
    let slug = feedback::make_slug(&feedback_message);
    let folder_name = format!("{timestamp}-{slug}");
    let feedback_dir = self.config.workspace.join("feedback").join(&folder_name);

    match feedback::save_feedback(
        &self.config.workspace,
        &feedback_dir,
        &self.db,
        &session_id,
        &feedback_message,
    )
    .await
    {
        Ok(name) => {
            info!(folder = %name, session_id = %session_id, "feedback saved");
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("Feedback saved to `feedback/{name}/`"))
                            .ephemeral(true),
                    ),
                )
                .await;
        }
        Err(e) => {
            error!("Failed to save feedback: {e}");
            let _ = command
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!("Failed to save feedback: {e}"))
                            .ephemeral(true),
                    ),
                )
                .await;
        }
    }
}
```

**Step 4: Run `just ci`**

**Step 5: Commit**

```
feat: wire /feedback slash command in Discord bot
```

---

### Task 4: Add `feedback/` to workspace bootstrap

**Files:**

- Modify: `src/config_workspace.rs:17-28`

**Step 1: Add `"feedback"` to the directory list**

```rust
for dir in [
    "skills",
    "agents",
    ".cache",
    "notes",
    "references",
    "diary",
    "projects",
    "shell",
    "feedback",
] {
```

**Step 2: Run `just ci`**

**Step 3: Commit**

```
chore: add feedback/ to workspace bootstrap dirs
```

---

### Task 5: Create the `fix-feedback` Claude Code skill

**Files:**

- Create: `.agents/skills/fix-feedback/SKILL.md`

**Step 1: Write the skill**

````markdown
---
name: fix-feedback
description: >-
  Triage and fix issues reported via Ghost's /feedback command. Use when the user points
  you to a feedback folder or asks you to fix a feedback report. Reads the pre-rendered
  feedback.md and transcript.md to understand and fix the issue.
---

# Fix Feedback

## Process

1. Read `feedback.md` in the feedback folder for the issue description and session ID
2. Read `transcript.md` for the last 10 messages with tool calls and results
3. Analyze the conversation: what the OPERATOR said, what GHOST did, what went wrong
4. Categorize the root cause:
   - **Bad tool use**: wrong tool chosen, bad parameters, missing tool
   - **Bad response**: tone, content, format issues
   - **UI problem**: Discord rendering, embed issues
   - **Prompt issue**: system prompt missing context, wrong instructions
   - **Code bug**: tool implementation, chat loop, provider issue
5. Locate the relevant source files and propose a fix
6. Implement the fix, run `just ci`

## Reading the transcript

`transcript.md` contains messages in chronological order. Pay attention to:

- Tool call names and arguments — did GHOST pick the right tool?
- Tool results — did the tool return what was expected?
- The sequence of messages leading up to the issue
- System messages that may have influenced behavior

If the transcript is insufficient, `ghost.db` is in the same folder. You can query it
with a Python script (use @uv-scripts conventions):

```python
import sqlite3
conn = sqlite3.connect("path/to/ghost.db")
# Query any table: session, message, knowledge, etc.
```
````

```

**Step 2: Commit**

```

feat: add fix-feedback Claude Code skill

```

---

### Task 6: Verify end-to-end

1. `just ci` passes
2. Deploy, use `/feedback this response was terrible` in Discord
3. Confirm folder created with `feedback.md`, `transcript.md`, `ghost.db`
4. Confirm `transcript.md` is readable and contains the right messages
5. Test the skill: point Claude Code at the folder, confirm it reads and analyzes
```
