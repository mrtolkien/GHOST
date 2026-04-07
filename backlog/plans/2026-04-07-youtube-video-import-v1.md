# YouTube Video Import v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import a single YouTube video URL as sectioned transcript references, then run
a dedicated agent to create notes from the imported video.

**Architecture:** Follow the existing two-step reference import flow.
`ghost convert youtube` fetches captions or falls back to CPU Whisper, splits the
transcript into bounded markdown sections in a staging directory, and prints provenance
metadata. `ghost reference import` then ingests the staging directory unchanged, while a
new `video-import` Lua agent reads the imported references and creates source and
concept notes.

**Tech Stack:** Rust CLI (`clap`), `serde_json`, `tokio::process::Command`, on-demand
nix shell tools (`yt-dlp`, `ffmpeg`, `whisper-cpp`), SQLite migration, Lua agent
prompts, existing reference import pipeline.

**Spec:** `backlog/tasks/4-import-v2/audio-content-import.md`

---

## File Map

| File                                               | Action | Responsibility                                                                                         |
| -------------------------------------------------- | ------ | ------------------------------------------------------------------------------------------------------ |
| `src/constants.rs`                                 | modify | Add tuneable limits for transcript section sizing and caption quality guards                           |
| `src/convert/mod.rs`                               | modify | Export `youtube` converter module                                                                      |
| `src/convert/youtube.rs`                           | create | YouTube metadata fetch, caption/STT acquisition, transcript parsing, section splitting, staging output |
| `src/cli/convert.rs`                               | modify | Add `ConvertCommand::Youtube` and CLI execution/printing                                               |
| `migrations/016_youtube_source_type.sql`           | create | Add `'youtube'` to `import_batch.source_type` CHECK constraint                                         |
| `src/reference_import/types.rs`                    | modify | Extend `ImportConfigJson` with YouTube metadata fields; allow `youtube` source snapshots               |
| `src/reference_import/topic.rs`                    | modify | Persist extended `_import.toml` fields for YouTube imports                                             |
| `assets/agents/video-import/agent.lua`             | create | Dedicated note-extraction agent for video references                                                   |
| `assets/agents/video-import/prompt.md`             | create | Single-shot prompt for shorter video imports                                                           |
| `assets/agents/video-import/prompt-progressive.md` | create | Progressive prompt for larger multi-section video imports                                              |
| `assets/skills/reference-import/skill.md`          | modify | Document `ghost convert youtube` and `video-import` workflow                                           |
| `tests/youtube_import.rs`                          | create | Converter/import tests and optional live integration coverage                                          |

---

## Task 1: Extend Provenance Schema for `youtube`

**Files:**

- Create: `migrations/016_youtube_source_type.sql`
- Modify: `src/reference_import/types.rs`
- Modify: `src/reference_import/topic.rs`

- [ ] **Step 1: Add migration `016_youtube_source_type.sql`**

Create a table-recreation migration following the existing `015_book_source_type.sql`
pattern, but add `'youtube'` to the `source_type` CHECK constraint:

```sql
-- Add 'youtube' to import_batch.source_type CHECK constraint.
PRAGMA foreign_keys=OFF;

CREATE TABLE import_batch_new (
    id TEXT PRIMARY KEY,
    topic_id TEXT NOT NULL UNIQUE REFERENCES topic(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'page', 'crawl', 'file', 'book', 'youtube')),
    source_url TEXT NOT NULL,
    version_ref TEXT,
    ref_count INTEGER NOT NULL DEFAULT 0,
    import_config TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO import_batch_new (id, topic_id, source_type, source_url, version_ref, ref_count, import_config, created_at, updated_at)
SELECT id, topic_id, source_type, source_url, version_ref, ref_count, import_config, created_at, updated_at
FROM import_batch;

DROP TABLE import_batch;
ALTER TABLE import_batch_new RENAME TO import_batch;

CREATE INDEX idx_import_batch_topic_id ON import_batch(topic_id);

PRAGMA foreign_key_check;
PRAGMA foreign_keys=ON;
```

- [ ] **Step 2: Extend `ImportConfigJson` with optional YouTube fields**

Add these optional fields to `src/reference_import/types.rs`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub video_id: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub channel: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub published_at: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub duration_seconds: Option<u64>,
#[serde(skip_serializing_if = "Option::is_none")]
pub transcript_source: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
pub section_count: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
pub chapter_count: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
pub language: Option<String>,
```

Update every `ImportConfigJson` constructor in the file to populate the new fields with
`None` unless the source is YouTube.

- [ ] **Step 3: Teach `ImportConfigJson::to_import_config()` to reject YouTube updates
      clearly**

Keep update support limited to `git` and `crawl`, but make the unsupported branch
explicit:

```rust
other => {
    return Err(ImportError::Config(format!(
        "unsupported source_type for update: {other}"
    )));
}
```

No update implementation is needed for YouTube v1.

- [ ] **Step 4: Extend `_import.toml` serialization**

In `src/reference_import/topic.rs`, add the new optional fields to the private
`ImportToml` struct and copy them from `ImportConfigJson` in `write_import_toml()`:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
video_id: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
channel: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
published_at: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
duration_seconds: Option<u64>,
#[serde(skip_serializing_if = "Option::is_none")]
transcript_source: Option<String>,
#[serde(skip_serializing_if = "Option::is_none")]
section_count: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
chapter_count: Option<usize>,
#[serde(skip_serializing_if = "Option::is_none")]
language: Option<String>,
```

- [ ] **Step 5: Add a focused serialization unit test**

At the bottom of `src/reference_import/topic.rs`, add a unit test that writes
`_import.toml` for a synthetic YouTube config and asserts it contains the expected extra
fields:

```rust
#[test]
fn write_import_toml_includes_youtube_fields() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let config = ImportConfigJson {
        source_type: "youtube".into(),
        source_url: "https://www.youtube.com/watch?v=test123".into(),
        git_ref: None,
        paths: vec![],
        extensions: vec![],
        max_depth: None,
        max_pages: None,
        title: Some("Test Video".into()),
        authors: None,
        language: Some("en".into()),
        publisher: None,
        publication_date: None,
        video_id: Some("test123".into()),
        channel: Some("Example Channel".into()),
        published_at: Some("2024-01-02".into()),
        duration_seconds: Some(1234),
        transcript_source: Some("auto".into()),
        section_count: Some(3),
        chapter_count: Some(1),
    };

    write_import_toml(tmp.path(), "videos/test", &config, None, 3).expect("write import toml");

    let content = std::fs::read_to_string(tmp.path().join("references/videos/test/_import.toml"))
        .expect("read toml");
    assert!(content.contains("source_type = \"youtube\""));
    assert!(content.contains("video_id = \"test123\""));
    assert!(content.contains("transcript_source = \"auto\""));
}
```

- [ ] **Step 6: Run targeted tests**

Run: `cargo test reference_import::topic --lib -- --nocapture`

Expected: the new `_import.toml` test passes.

- [ ] **Step 7: Commit**

```bash
git add migrations/016_youtube_source_type.sql src/reference_import/types.rs src/reference_import/topic.rs
git commit -m "feat: add youtube import provenance schema"
```

---

## Task 2: Add YouTube Converter Core

**Files:**

- Create: `src/convert/youtube.rs`
- Modify: `src/convert/mod.rs`
- Modify: `src/constants.rs`

- [ ] **Step 1: Add tuneable constants to `src/constants.rs`**

Add named constants near the other import-related values:

```rust
/// Maximum characters allowed in a single YouTube transcript section.
pub const YOUTUBE_SECTION_MAX_CHARS: usize = 40_000;

/// Minimum characters required for a usable imported transcript.
pub const YOUTUBE_MIN_TRANSCRIPT_CHARS: usize = 500;

/// Minimum characters to keep a section after splitting.
pub const YOUTUBE_MIN_SECTION_CHARS: usize = 1_500;
```

- [ ] **Step 2: Export the new module**

Update `src/convert/mod.rs`:

```rust
pub mod crawl;
pub mod epub;
pub mod error;
pub mod git;
pub mod pdf;
pub mod staging;
pub mod youtube;
```

- [ ] **Step 3: Create converter types in `src/convert/youtube.rs`**

Start with focused types:

```rust
#[derive(Debug)]
#[must_use]
pub struct YoutubeConvertResult {
    pub staging_dir: PathBuf,
    pub metadata: YoutubeMetadata,
    pub section_count: usize,
    pub chapter_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YoutubeMetadata {
    pub source_url: String,
    pub video_id: String,
    pub title: Option<String>,
    pub channel: Option<String>,
    pub published_at: Option<String>,
    pub duration_seconds: Option<u64>,
    pub language: Option<String>,
    pub transcript_source: TranscriptSource,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptSource {
    Manual,
    Auto,
    Whisper,
}
```

- [ ] **Step 4: Add transcript parsing and sectioning helpers with unit tests first**

Write pure helper functions before the command runner:

```rust
fn split_sections(
    cues: &[TranscriptCue],
    chapter_starts: &[u64],
    max_chars: usize,
    min_chars: usize,
) -> Vec<TranscriptSection> { /* ... */ }

fn timestamp_slug(seconds: u64) -> String {
    format!("{:02}{:02}", seconds / 60, seconds % 60)
}
```

Add unit tests in the same file for:

- no chapters -> length-based sections
- oversized chapter -> split into multiple sections
- tiny adjacent chapters -> merged section
- stable filename prefix from start timestamp

Example test:

```rust
#[test]
fn split_sections_enforces_max_chars_without_chapters() {
    let cues = vec![
        TranscriptCue::new(0, "a".repeat(25_000)),
        TranscriptCue::new(60, "b".repeat(25_000)),
    ];

    let sections = split_sections(
        &cues,
        &[],
        crate::constants::YOUTUBE_SECTION_MAX_CHARS,
        crate::constants::YOUTUBE_MIN_SECTION_CHARS,
    );

    assert_eq!(sections.len(), 2);
    assert!(sections.iter().all(|s| s.text.len() <= crate::constants::YOUTUBE_SECTION_MAX_CHARS));
}
```

- [ ] **Step 5: Implement on-demand nix command helpers**

Add small helpers that shell out through `nix shell` for rare tools:

```rust
async fn run_yt_dlp_json(url: &str) -> Result<String, ConvertError> { /* nix shell nixpkgs#yt-dlp --command yt-dlp ... */ }

async fn run_yt_dlp_subtitles(url: &str, output_dir: &Path) -> Result<Vec<PathBuf>, ConvertError> { /* write subtitle files */ }

async fn run_yt_dlp_audio(url: &str, output_dir: &Path) -> Result<PathBuf, ConvertError> { /* download audio only */ }

async fn run_whisper(audio_path: &Path, output_dir: &Path) -> Result<PathBuf, ConvertError> { /* nix shell nixpkgs#whisper-cpp --command whisper-cli ... */ }
```

Use `tokio::process::Command`, capture stderr, and convert failures into actionable
`ConvertError::Conversion` messages that name the acquisition path that failed.

- [ ] **Step 6: Implement transcript acquisition priority**

Implement `convert_youtube()` with the approved order:

```rust
pub async fn convert_youtube(
    staging_root: &Path,
    url: &str,
) -> Result<YoutubeConvertResult, ConvertError> {
    // 1. validate single-video URL
    // 2. fetch metadata via yt-dlp --dump-single-json
    // 3. try manual subtitles
    // 4. else auto subtitles
    // 5. else audio-only + whisper
    // 6. split transcript into sections
    // 7. write markdown files + metadata json
}
```

Validation rules:

- reject playlist URLs (`list=` query parameter)
- reject channel/user URLs
- require a recoverable video id

- [ ] **Step 7: Write staging output**

Persist:

- one markdown file per section
- `_metadata.json` for the converter result

Do **not** create `_originals/`.

Each section file should begin with lightweight source context:

```md
# <section title or fallback>

Video: <title> URL: <source_url> Start: 08:40

<transcript text>
```

Keep the body content transcript-first; do not add AI summaries in convert.

- [ ] **Step 8: Add focused unit tests for metadata and error paths**

Add tests for:

- invalid playlist URL rejected
- transcript shorter than `YOUTUBE_MIN_TRANSCRIPT_CHARS` rejected
- metadata JSON serialization includes transcript source

- [ ] **Step 9: Run converter tests**

Run: `cargo test convert::youtube --lib -- --nocapture`

Expected: the pure parsing/sectioning/unit tests pass without network access.

- [ ] **Step 10: Commit**

```bash
git add src/constants.rs src/convert/mod.rs src/convert/youtube.rs
git commit -m "feat: add youtube transcript converter"
```

---

## Task 3: Wire the `ghost convert youtube` CLI

**Files:**

- Modify: `src/cli/convert.rs`

- [ ] **Step 1: Add `Youtube` command variant**

Extend `ConvertCommand`:

```rust
/// Convert a single YouTube video to sectioned markdown transcript files
Youtube {
    /// Individual YouTube video URL
    #[arg(long)]
    url: String,
    /// Output directory for staging (default: <workspace>/.staging)
    #[arg(long)]
    output: Option<PathBuf>,
}
```

- [ ] **Step 2: Add the execute arm**

Insert a new `match` branch:

```rust
ConvertCommand::Youtube { url, output } => {
    let staging_root = staging_root(&workspace, output.as_deref());
    let result = crate::convert::youtube::convert_youtube(&staging_root, &url)
        .await
        .map_err(convert_err)?;
    print_youtube_result(&result);
    Ok(())
}
```

- [ ] **Step 3: Add `print_youtube_result()`**

Print the staging path and machine-readable fields used by follow-up tooling:

```rust
fn print_youtube_result(result: &YoutubeConvertResult) {
    println!("{}", result.staging_dir.display());
    println!("source_type=youtube");
    println!("source_url={}", result.metadata.source_url);
    println!("video_id={}", result.metadata.video_id);
    if let Some(title) = &result.metadata.title {
        println!("title={title}");
    }
    if let Some(channel) = &result.metadata.channel {
        println!("channel={channel}");
    }
    println!("transcript_source={:?}", result.metadata.transcript_source);
    println!("sections={}", result.section_count);
    println!("chapters={}", result.chapter_count);
}
```

Change the transcript-source print to a stable lowercase string before finalizing the
code; do not leave the debug formatter in place.

- [ ] **Step 4: Add a small CLI smoke test if the file already has test coverage**

If `src/cli/convert.rs` already has tests by the time implementation reaches this step,
add one for `print_youtube_result()`. Otherwise rely on the converter/import integration
tests and keep this file focused.

- [ ] **Step 5: Run a compile check**

Run: `cargo check`

Expected: the new CLI variant compiles and links cleanly.

- [ ] **Step 6: Commit**

```bash
git add src/cli/convert.rs
git commit -m "feat: add ghost convert youtube command"
```

---

## Task 4: Add End-to-End Import Tests

**Files:**

- Create: `tests/youtube_import.rs`

- [ ] **Step 1: Create a non-network unit/integration test around staging import**

Write a test that synthesizes a converted YouTube staging directory, then verifies the
generic importer and `_import.toml` behavior:

```rust
#[tokio::test]
async fn youtube_staging_import_writes_references_and_metadata() {
    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);
    let staging_dir = workspace_path.join(".staging/test-video");
    std::fs::create_dir_all(&staging_dir).expect("create staging dir");

    std::fs::write(staging_dir.join("01-0000-intro.md"), "# Intro\n\nTranscript...").expect("write section");
    std::fs::write(staging_dir.join("02-0840-main.md"), "# Main\n\nTranscript...").expect("write section");

    let provenance = ImportProvenance {
        source_type: Some("youtube".into()),
        source_url: Some("https://www.youtube.com/watch?v=test123".into()),
        version_ref: None,
        git_ref: None,
    };

    let result = import_from_path(&db, workspace_path, &staging_dir, "videos/test-video", &provenance, None)
        .await
        .expect("import succeeds");

    assert_eq!(result.references_created, 2);
}
```

- [ ] **Step 2: Add assertions for YouTube-specific `_import.toml`**

After import, assert:

- topic exists
- reference files exist on disk
- `_import.toml` exists
- it contains `source_type = "youtube"`

- [ ] **Step 3: Add an opt-in live conversion test**

Add a live test gated behind an env var and `live-tests` feature:

```rust
#[cfg(feature = "live-tests")]
#[tokio::test]
async fn youtube_convert_live_from_env_url() {
    let Ok(url) = std::env::var("GHOST_TEST_YOUTUBE_URL") else {
        eprintln!("skipping youtube live test: GHOST_TEST_YOUTUBE_URL not set");
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let result = ghost::convert::youtube::convert_youtube(tmp.path(), &url)
        .await
        .expect("convert_youtube should succeed");

    assert!(result.section_count >= 1);
    assert!(result.staging_dir.exists());
}
```

This keeps CI deterministic while still giving maintainers a real pipeline test.

- [ ] **Step 4: Run the non-live test file**

Run: `cargo test youtube_import -- --nocapture`

Expected: synthetic staging import test passes locally without network access.

- [ ] **Step 5: Commit**

```bash
git add tests/youtube_import.rs
git commit -m "test: add youtube import coverage"
```

---

## Task 5: Add the `video-import` Agent

**Files:**

- Create: `assets/agents/video-import/agent.lua`
- Create: `assets/agents/video-import/prompt.md`
- Create: `assets/agents/video-import/prompt-progressive.md`

- [ ] **Step 1: Create `agent.lua` by adapting the `book-import` pattern**

Use the same manifest-vs-single-shot strategy, but tune names and copy for videos:

```lua
local template = require("ghost.template")

local MAX_SINGLE_SHOT_BYTES = 640000

return {
    name = "video-import",
    description = "Create structured notes from imported video transcript sections",
    max_iterations = 200,
    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
        "shell",
    },
    skills = { "note-writer" },
    build = function(ctx, args)
        local topic = args.topic or error("video-import requires args.topic")
        local title = args.title or "Unknown"
        local channel = args.channel or "Unknown"
        -- list transcript files, decide single-shot vs progressive, render prompt
    end,
}
```

Use `references/<topic>/*.md`, skip metadata files, and preserve the same progressive
fallback pattern as `book-import`.

- [ ] **Step 2: Write `prompt.md` for smaller imports**

Create a prompt that tells the agent to:

- search for existing notes first
- create one source note for the video
- create 1-3 concept notes
- use timestamps only when they materially improve the note
- avoid turning every section into its own note

Use typed wiki links and `sources: ["videos/<slug>"]`.

- [ ] **Step 3: Write `prompt-progressive.md` for larger imports**

The progressive prompt must explicitly instruct the agent to:

- read transcript sections one by one with `file_read`
- keep a running mental model of the thesis/structure
- only write notes after enough evidence is gathered

- [ ] **Step 4: Add an optional live agent integration test**

If the live YouTube convert test is stable enough during implementation, extend
`tests/youtube_import.rs` with an LLM-gated test mirroring `epub_agent_creates_notes()`:

```rust
#[cfg(feature = "live-tests-llms")]
#[tokio::test]
async fn youtube_agent_creates_notes() { /* convert -> import -> run video-import -> assert source note exists */ }
```

Skip this step if the live conversion fixture proves too brittle; do not block the whole
feature on a flaky network test.

- [ ] **Step 5: Commit**

```bash
git add assets/agents/video-import/agent.lua assets/agents/video-import/prompt.md assets/agents/video-import/prompt-progressive.md tests/youtube_import.rs
git commit -m "feat: add video import note agent"
```

---

## Task 6: Update Operator-Facing Import Workflow Docs

**Files:**

- Modify: `assets/skills/reference-import/skill.md`

- [ ] **Step 1: Add a YouTube import section**

Document the exact two-step workflow:

```sh
ghost convert youtube --url https://www.youtube.com/watch?v=<id>
ghost reference import /path/to/.staging/<slug> --topic videos/<slug> --source-type youtube
```

- [ ] **Step 2: Document transcript priority and fallback**

Add concise rules:

- manual captions first
- auto captions second
- CPU Whisper fallback only when captions are missing
- single video URLs only in v1

- [ ] **Step 3: Document note creation**

Add the `video-import` agent example:

```json agent
{
  "action": "start",
  "agent": "video-import",
  "prompt": "Create notes from the imported video.",
  "args": {
    "topic": "videos/<slug>",
    "title": "<title from convert output>",
    "channel": "<channel from convert output>"
  }
}
```

- [ ] **Step 4: Run formatting and docs-adjacent checks**

Run: `just fmt`

Expected: markdown formatting remains clean and no generated formatting noise appears
outside the intended files.

- [ ] **Step 5: Commit**

```bash
git add assets/skills/reference-import/skill.md
git commit -m "docs: add youtube reference import workflow"
```

---

## Task 7: Final Verification

**Files:**

- Modify: none

- [ ] **Step 1: Run targeted Rust tests**

Run:

```bash
cargo test convert::youtube --lib -- --nocapture
cargo test youtube_import -- --nocapture
```

Expected: converter unit tests and synthetic import coverage pass.

- [ ] **Step 2: Run full project verification**

Run: `just ci`

Expected: fmt, check, clippy, and tests pass with zero warnings.

- [ ] **Step 3: Optional live verification**

If `GHOST_TEST_YOUTUBE_URL` is set and the machine has nix/network access, run:

```bash
cargo test --features live-tests youtube_convert_live_from_env_url -- --nocapture
```

Expected: converter fetches captions or Whisper fallback and produces at least one
staged section.

- [ ] **Step 4: Final commit**

```bash
git status --short
```

Expected: only intended YouTube import files are modified. If clean and the work is not
already split across earlier commits, make a final integration commit:

```bash
git add src/constants.rs src/convert/mod.rs src/convert/youtube.rs src/cli/convert.rs migrations/016_youtube_source_type.sql src/reference_import/types.rs src/reference_import/topic.rs assets/agents/video-import/agent.lua assets/agents/video-import/prompt.md assets/agents/video-import/prompt-progressive.md assets/skills/reference-import/skill.md tests/youtube_import.rs
git commit -m "feat: add youtube video import pipeline"
```

---

## Self-Review

Spec coverage check:

- single video URL only: Task 2 URL validation
- transcript priority manual → auto → Whisper: Task 2 acquisition logic
- no GPU path in v1: Task 2 on-demand CPU Whisper helper only
- split primarily by length with chapter hints: Task 2 pure sectioning helpers
- 40k character section ceiling: Task 2 constants and splitter tests
- `_import.toml` metadata: Task 1 provenance extensions
- generic import path unchanged: Task 4 synthetic staging import coverage
- dedicated `video-import` agent: Task 5
- operator workflow docs: Task 6

Placeholder scan:

- No `TODO`/`TBD` markers remain.
- Live network testing is explicitly optional and gated, not a hidden dependency for
  core verification.

Type consistency:

- Use `youtube` as the provenance `source_type` everywhere.
- Use `TranscriptSource` only inside converter code; persist lowercase strings in
  metadata output.
