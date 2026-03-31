# EPUB Book Import — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use
> superpowers:subagent-driven-development (recommended) or superpowers:executing-plans
> to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Import EPUB books as per-chapter references, then run a Lua agent to generate
structured notes (source note, author note, concept/theme notes).

**Architecture:** Two-step flow matching existing converters — `ghost convert epub`
produces a staging directory of per-chapter markdown files, then
`ghost reference import` indexes them. A bundled `book-import` Lua agent reads the
imported chapters and creates linked notes. New `'book'` source type requires a SQLite
migration.

**Tech Stack:** rbook (EPUB parsing), htmd (HTML→markdown, already a dependency), SQLite
migration, Lua agent with note_write tool.

**Spec:** `backlog/tasks/5-import-v2/02-epub-import.md`

**Test EPUB:** `/home/tolki/Documents/books/Animal Farm.epub` — 13 spine items
(titlepage + title + 11 chapters), metadata: title "Animal Farm", author "George
Orwell", publisher "Secker & Warburg", date "1945-08-02", language "en".

---

## File Map

| File                                      | Action | Responsibility                                          |
| ----------------------------------------- | ------ | ------------------------------------------------------- |
| `Cargo.toml`                              | modify | Add `rbook` dependency                                  |
| `src/convert/epub.rs`                     | create | `EpubMetadata`, `EpubConvertResult`, `convert_epub()`   |
| `src/convert/mod.rs`                      | modify | Add `pub mod epub;`                                     |
| `src/cli/convert.rs`                      | modify | `ConvertCommand::Epub` variant + execute arm            |
| `migrations/015_book_source_type.sql`     | create | Add `'book'` to CHECK constraint                        |
| `src/reference_import/types.rs`           | modify | Book fields on `ImportConfigJson`, `ImportSource::Book` |
| `assets/agents/book-import/agent.lua`     | create | Agent definition                                        |
| `assets/agents/book-import/prompt.md`     | create | Agent system prompt                                     |
| `assets/skills/reference-import/skill.md` | modify | Add book import section                                 |
| `tests/epub_import.rs`                    | create | E2E test (convert + import + agent)                     |

---

## Task 1: Add `rbook` Dependency

**Files:**

- Modify: `Cargo.toml`

- [ ] **Step 1: Add rbook to Cargo.toml**

Add after the `reqwest` line (alphabetical in that section):

```toml
rbook = "0.7"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check 2>&1 | tail -5` Expected: compiles successfully (rbook is a pure Rust
crate, no system deps)

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add rbook dependency for EPUB parsing"
```

---

## Task 2: Create `src/convert/epub.rs` — Types and Conversion

**Files:**

- Create: `src/convert/epub.rs`
- Modify: `src/convert/mod.rs`

This is the core conversion logic. It follows the same pattern as `src/convert/pdf.rs`:
create staging dir via `staging::create_staging_dir`, write output files, copy original
to `_originals/`.

**rbook API reference** (from docs.rs):

- `rbook::Epub::open(path)` — open EPUB file
- `epub.metadata().title()?.value()` — title string
- `epub.metadata().creators()` — iterator of creators
- `epub.metadata().published()?.date()` — publication date
- `epub.reader()` + `reader.read_next()` — spine iteration (reading order)
- `data.content()` — XHTML string content of spine item
- `data.manifest_entry().href()` — resource path within EPUB

**htmd pattern** (from `src/web/fetch.rs:328-337`):

```rust
htmd::HtmlToMarkdown::builder()
    .skip_tags(vec!["script", "style", "nav", "footer", "header", "noscript", "svg", "iframe"])
    .build()
```

- [ ] **Step 1: Add `pub mod epub;` to `src/convert/mod.rs`**

```rust
pub mod crawl;
pub mod epub;
pub mod error;
pub mod git;
pub mod pdf;
pub mod staging;
```

- [ ] **Step 2: Create `src/convert/epub.rs` with types**

```rust
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::error::ConvertError;
use super::staging::{create_staging_dir, slug_from_source};

/// Subdirectory within the staging dir for preserving original source files.
const ORIGINALS_SUBDIR: &str = "_originals";

/// Metadata file written to staging dir for downstream import.
const METADATA_FILE: &str = "_metadata.json";

/// Minimum content length (bytes, after trim) to keep a spine item.
/// Shorter items are title pages, blank separators, etc.
const MIN_CONTENT_BYTES: usize = 50;

/// Result of converting an EPUB file into a staging directory.
#[derive(Debug)]
#[must_use]
pub struct EpubConvertResult {
    /// Path to the staging directory containing per-chapter markdown files.
    pub staging_dir: PathBuf,
    /// Number of chapter files written (excludes trivial spine items).
    pub chapter_count: usize,
    /// Extracted book metadata.
    pub metadata: EpubMetadata,
}

/// Metadata extracted from the EPUB's OPF package document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpubMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
}
```

- [ ] **Step 3: Implement `convert_epub`**

```rust
/// Convert an EPUB file to per-chapter markdown files in a staging directory.
///
/// Each spine item (reading-order entry) becomes a separate markdown file.
/// Trivial items (< [`MIN_CONTENT_BYTES`] after conversion) are skipped.
/// The original EPUB is preserved in `_originals/`.
#[tracing::instrument(
    name = "convert_epub",
    skip_all,
    fields(path = %epub_path.display())
)]
pub fn convert_epub(
    staging_root: &Path,
    epub_path: &Path,
) -> Result<EpubConvertResult, ConvertError> {
    if !epub_path.exists() {
        return Err(ConvertError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", epub_path.display()),
        )));
    }

    let epub = rbook::Epub::open(epub_path)
        .map_err(|e| ConvertError::Conversion(format!("failed to open EPUB: {e}")))?;

    let metadata = extract_metadata(&epub);

    let slug = slug_from_source(&epub_path.to_string_lossy());
    let staging_dir = create_staging_dir(staging_root, &slug)?;

    // Build HTML→markdown converter (same config as web/fetch.rs)
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "footer", "header", "noscript", "svg", "iframe",
        ])
        .build();

    // Iterate spine items in reading order
    let mut reader = epub.reader();
    let mut chapter_idx: usize = 0;
    let mut chapter_count: usize = 0;
    let title_lower = metadata.title.as_deref().unwrap_or("").to_lowercase();

    while let Some(content_result) = reader.read_next() {
        let data = content_result
            .map_err(|e| ConvertError::Conversion(format!("failed to read spine item: {e}")))?;

        let xhtml = data.content();
        let markdown = converter
            .convert(xhtml)
            .unwrap_or_else(|_| xhtml.to_string());

        // Strip book title if it appears as the first line
        let markdown = strip_title_line(&markdown, &title_lower);
        let trimmed = markdown.trim();

        if trimmed.len() < MIN_CONTENT_BYTES {
            chapter_idx += 1;
            continue;
        }

        // Derive filename from spine item href or fall back to index
        let href = data.manifest_entry().href();
        let filename = chapter_filename(chapter_idx, href.to_string().as_str());

        std::fs::write(staging_dir.join(&filename), trimmed)?;
        chapter_count += 1;
        chapter_idx += 1;
    }

    if chapter_count == 0 {
        return Err(ConvertError::Conversion(
            "EPUB produced no chapter content".into(),
        ));
    }

    // Preserve original EPUB
    let originals_dir = staging_dir.join(ORIGINALS_SUBDIR);
    std::fs::create_dir_all(&originals_dir)?;
    if let Some(filename) = epub_path.file_name() {
        std::fs::copy(epub_path, originals_dir.join(filename))?;
    }

    // Write metadata for downstream import
    let metadata_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| ConvertError::Conversion(format!("failed to serialize metadata: {e}")))?;
    std::fs::write(staging_dir.join(METADATA_FILE), metadata_json)?;

    Ok(EpubConvertResult {
        staging_dir,
        chapter_count,
        metadata,
    })
}

/// Extract metadata from the EPUB's OPF package document.
fn extract_metadata(epub: &rbook::Epub) -> EpubMetadata {
    let meta = epub.metadata();

    let title = meta.title().ok().map(|t| t.value().to_string());

    let authors: Vec<String> = meta
        .creators()
        .map(|c| c.value().to_string())
        .collect();

    let language = meta.languages().next().map(|l| l.value().to_string());

    // publisher and date are often missing — extract best-effort
    let publisher = meta
        .contributors()
        .find(|c| {
            c.main_role()
                .map(|r| r.code() == "pbl")
                .unwrap_or(false)
        })
        .map(|c| c.value().to_string())
        .or_else(|| {
            meta.publishers().next().map(|p| p.value().to_string())
        });

    let publication_date = meta
        .published()
        .ok()
        .map(|d| format!("{}", d.date()));

    EpubMetadata {
        title,
        authors,
        language,
        publisher,
        publication_date,
    }
}

/// Strip the book title if it appears as the first non-empty line.
///
/// htmd often includes the `<title>` tag content as the first line of each
/// chapter, producing duplicated title text.
fn strip_title_line<'a>(markdown: &'a str, title_lower: &str) -> &'a str {
    if title_lower.is_empty() {
        return markdown;
    }
    // Find the first non-empty line
    let trimmed = markdown.trim_start();
    let first_line_end = trimmed.find('\n').unwrap_or(trimmed.len());
    let first_line = trimmed[..first_line_end].trim();

    // Strip markdown heading prefix if present
    let bare = first_line.trim_start_matches('#').trim();

    if bare.to_lowercase() == title_lower {
        trimmed[first_line_end..].trim_start_matches('\n')
    } else {
        markdown
    }
}

/// Generate a chapter filename from the spine item index and EPUB href.
///
/// Produces `{idx:02}-{sanitized_stem}.md` or `{idx:02}-chapter-{idx}.md`
/// as fallback.
fn chapter_filename(idx: usize, href: &str) -> String {
    let stem = Path::new(href)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");

    let sanitized: String = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect();

    let name = if sanitized.is_empty() || sanitized.chars().all(|c| c == '-') {
        format!("chapter-{idx}")
    } else {
        sanitized.trim_matches('-').to_lowercase()
    };

    format!("{idx:02}-{name}.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_title_line_removes_matching_title() {
        let md = "Animal Farm\n\nChapter content here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, "Chapter content here.");
    }

    #[test]
    fn strip_title_line_removes_heading_title() {
        let md = "# Animal Farm\n\nChapter content here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, "Chapter content here.");
    }

    #[test]
    fn strip_title_line_preserves_non_matching() {
        let md = "Chapter I\n\nContent here.";
        let result = strip_title_line(md, "animal farm");
        assert_eq!(result, md);
    }

    #[test]
    fn strip_title_line_empty_title() {
        let md = "Whatever\n\nContent.";
        let result = strip_title_line(md, "");
        assert_eq!(result, md);
    }

    #[test]
    fn chapter_filename_from_href() {
        assert_eq!(chapter_filename(0, "OEBPS/title.xhtml"), "00-title.md");
        assert_eq!(chapter_filename(2, "OEBPS/part1.xhtml"), "02-part1.md");
        assert_eq!(chapter_filename(10, "chapter-x.html"), "10-chapter-x.md");
    }

    #[test]
    fn chapter_filename_fallback() {
        assert_eq!(chapter_filename(3, ""), "03-chapter-3.md");
        assert_eq!(chapter_filename(5, "///"), "05-chapter-5.md");
    }
}
```

**Note on `extract_metadata`:** The rbook API may differ slightly from the docs.rs
summary — the implementer should check exact method signatures at compile time and
adapt. The important contract is: extract title, authors, language, publisher, date as
`Option<String>` / `Vec<String>`. If a particular metadata accessor doesn't exist (e.g.
`publishers()`), omit that field gracefully.

- [ ] **Step 4: Run unit tests**

Run: `cargo test convert::epub --lib -- --nocapture` Expected: all 6 tests pass

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: passes (no clippy warnings, no format issues)

- [ ] **Step 6: Commit**

```bash
git add src/convert/epub.rs src/convert/mod.rs
git commit -m "feat: EPUB to markdown converter (rbook + htmd)"
```

---

## Task 3: Wire CLI — `ghost convert epub`

**Files:**

- Modify: `src/cli/convert.rs`

- [ ] **Step 1: Add `Epub` variant to `ConvertCommand`**

After the `Crawl` variant (line ~70 in `src/cli/convert.rs`), add:

```rust
    /// Convert an EPUB book to per-chapter markdown files
    Epub {
        /// Path to the EPUB file
        #[arg(long)]
        path: String,
        /// Output directory for staging (default: <workspace>/.staging)
        #[arg(long)]
        output: Option<PathBuf>,
    },
```

- [ ] **Step 2: Add import for `EpubConvertResult`**

At the top of the file, add to the imports:

```rust
use crate::convert::epub::EpubConvertResult;
```

- [ ] **Step 3: Add match arm in `execute()`**

After the `ConvertCommand::Crawl` arm, add:

```rust
        ConvertCommand::Epub { path, output } => {
            let staging_root = staging_root(&workspace, output.as_deref());

            let result = crate::convert::epub::convert_epub(
                &staging_root,
                std::path::Path::new(&path),
            )
            .map_err(convert_err)?;

            print_epub_result(&result);
            Ok(())
        }
```

- [ ] **Step 4: Add `print_epub_result` function**

After `print_crawl_result`:

```rust
/// Print stdout metadata for an EPUB conversion result.
fn print_epub_result(result: &EpubConvertResult) {
    println!("{}", result.staging_dir.display());
    println!("source_type=book");
    if let Some(title) = &result.metadata.title {
        println!("title={title}");
    }
    if !result.metadata.authors.is_empty() {
        println!("authors={}", result.metadata.authors.join(", "));
    }
    println!("chapters={}", result.chapter_count);
}
```

- [ ] **Step 5: Run `just ci`**

Run: `just ci` Expected: passes

- [ ] **Step 6: Commit**

```bash
git add src/cli/convert.rs
git commit -m "feat: ghost convert epub CLI command"
```

---

## Task 4: Migration — Add `'book'` Source Type

**Files:**

- Create: `migrations/015_book_source_type.sql`

SQLite doesn't support `ALTER TABLE ... ALTER CONSTRAINT`. The standard approach is to
recreate the table with the updated CHECK constraint.

- [ ] **Step 1: Create migration file**

Create `migrations/015_book_source_type.sql`:

```sql
-- Add 'book' to import_batch.source_type CHECK constraint.
-- SQLite requires table recreation to modify CHECK constraints.

CREATE TABLE import_batch_new (
    id TEXT PRIMARY KEY NOT NULL,
    topic_id TEXT NOT NULL REFERENCES topic(id) ON DELETE CASCADE,
    source_type TEXT NOT NULL CHECK (source_type IN ('git', 'page', 'crawl', 'file', 'book')),
    source_url TEXT NOT NULL,
    version_ref TEXT,
    ref_count INTEGER NOT NULL DEFAULT 0,
    import_config TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(topic_id)
);

INSERT INTO import_batch_new SELECT * FROM import_batch;
DROP TABLE import_batch;
ALTER TABLE import_batch_new RENAME TO import_batch;
```

- [ ] **Step 2: Verify migration applies**

Run: `cargo test db -- --nocapture 2>&1 | tail -20` Expected: tests that create
databases apply migrations successfully

- [ ] **Step 3: Commit**

```bash
git add migrations/015_book_source_type.sql
git commit -m "feat: add 'book' source type to import_batch CHECK constraint"
```

---

## Task 5: Extend Import Types for Books

**Files:**

- Modify: `src/reference_import/types.rs`

- [ ] **Step 1: Add book metadata fields to `ImportConfigJson`**

In `src/reference_import/types.rs`, add these fields to the `ImportConfigJson` struct
(after `max_pages`):

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authors: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publication_date: Option<String>,
```

- [ ] **Step 2: Add `Book` variant to `ImportSource`**

After the `File` variant:

```rust
    Book {
        path: String,
        title: Option<String>,
        authors: Vec<String>,
    },
```

- [ ] **Step 3: Update `From<&ImportConfig> for ImportConfigJson`**

Add a new arm in the `match &config.source` block:

```rust
            ImportSource::Book {
                path,
                title,
                authors,
            } => ImportConfigJson {
                source_type: "book".into(),
                source_url: path.clone(),
                git_ref: None,
                paths: vec![],
                extensions: vec![],
                max_depth: None,
                max_pages: None,
                title: title.clone(),
                authors: Some(authors.clone()),
                language: None,
                publisher: None,
                publication_date: None,
            },
```

- [ ] **Step 4: Update existing arms with new fields**

Add the five new fields (all `None`) to each existing arm (`Git`, `Crawl`, `File`) in
the `From` impl. Example for `Git`:

```rust
                title: None,
                authors: None,
                language: None,
                publisher: None,
                publication_date: None,
```

- [ ] **Step 5: Update `ImportConfigJson` default construction in `import.rs`**

In `src/reference_import/import.rs`, function `upsert_provenance` (~line 211), the
`ImportConfigJson` struct literal needs the five new fields:

```rust
    let config_json = ImportConfigJson {
        source_type: source_type.clone(),
        source_url: source_url.clone(),
        git_ref: provenance.git_ref.clone(),
        paths: vec![],
        extensions: vec![],
        max_depth: None,
        max_pages: None,
        title: None,
        authors: None,
        language: None,
        publisher: None,
        publication_date: None,
    };
```

- [ ] **Step 6: Run `just ci`**

Run: `just ci` Expected: passes — the new fields are all `Option` with
`skip_serializing_if`, so existing serialization/deserialization is unaffected.

- [ ] **Step 7: Commit**

```bash
git add src/reference_import/types.rs src/reference_import/import.rs
git commit -m "feat: extend import types with book metadata fields"
```

---

## Task 6: Book Import Agent

**Files:**

- Create: `assets/agents/book-import/agent.lua`
- Create: `assets/agents/book-import/prompt.md`

The agent is bundled into the binary automatically by `build.rs` (it scans `assets/`).
It gets installed to the workspace via `bundled::install_all()`.

- [ ] **Step 1: Create `assets/agents/book-import/agent.lua`**

```lua
local template = require("ghost.template")

return {
    name = "book-import",
    description = "Create structured notes from imported book chapters",

    max_iterations = 40,
    reasoning_effort = "high",

    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
        "shell",
    },

    skills = { "note-writer" },

    build = function(ctx, args)
        local topic = args.topic or error("book-import requires args.topic")
        local title = args.title or "Unknown"
        local authors = args.authors or "Unknown"

        local note_skill = load_skill("note-writer")
        local system_prompt = template.render(read_file("prompt.md"), {
            note_skill = note_skill,
        })

        local user_message = "Create notes for the following book.\n\n"
            .. "**Title**: " .. title .. "\n"
            .. "**Author(s)**: " .. authors .. "\n"
            .. "**Topic (reference path)**: " .. topic .. "\n\n"
            .. "Chapters are at `references/" .. topic .. "/`. "
            .. "Start by listing the files, then read them all."

        return {
            system_prompt = system_prompt,
            messages = { { role = "user", content = user_message } },
        }
    end,
}
```

- [ ] **Step 2: Create `assets/agents/book-import/prompt.md`**

```markdown
You are a scholarly research assistant creating structured knowledge notes from an
imported book. You have access to the full text of the book as reference files.

## Your Task

1. **Read every chapter** — list the files at the reference path, then read each one in
   full. You need the complete text to write accurate notes.

2. **Determine genre** — is this fiction or non-fiction? This changes your approach:
   - **Non-fiction**: focus on the **logic and argumentation**. What is the thesis? What
     evidence supports it? What frameworks does the author introduce?
   - **Fiction**: focus on **themes** — the big ideas the work explores. Not plot
     summaries. Think literary analysis, not book report.

3. **Search existing knowledge** — before creating any note, search for existing notes
   about the author, the concepts, the historical period, related works. Update existing
   notes rather than duplicating. Link generously.

4. **Create the source note** — one `source` archetype note for the book itself:
   - Title: the book's title (e.g., "Animal Farm")
   - Structured summary: central thesis/narrative arc, key arguments or themes,
     structure
   - This is the hub — all other notes link back to it
   - Can be longer than typical notes (up to ~800 words)
   - Tag: `books/{genre}` (e.g., `books/fiction`, `books/economics`)

5. **Create or update the author note** — `entity` archetype:
   - If an author note already exists, update it with a link to this book
   - If not, create one: key biographical facts relevant to understanding their work,
     other notable works, intellectual tradition
   - Link: `[[wrote>Book Title]]`
   - Tag: `people/authors`

6. **Create secondary concept notes** — for major ideas, themes, or frameworks:
   - Each is a standalone `entity` or `analysis` note
   - Non-fiction: capture the book's key arguments and frameworks as `analysis` notes
   - Fiction: capture themes (power, corruption, freedom, etc.) as `entity` notes
   - Link back: `[[from>Book Title]]` and `[[by>Author Name]]`
   - Link to any existing notes about related concepts
   - Only create notes for concepts substantial enough to stand alone — don't fragment
     into tiny stubs
   - 2-5 concept notes is typical; don't force more

## Linking Strategy

Think like Wikipedia. Every note should be densely linked:

- `[[about>Theme]]` — what this note is about
- `[[by>Author]]` — who wrote it
- `[[from>Book Title]]` — source attribution
- `[[compares>Other Work]]` — comparative references
- `[[influenced_by>Earlier Work]]` — intellectual lineage
- `[[wrote>Book Title]]` — on author notes
- `[[explores>Theme]]` — on source notes

Search before linking — if a note about a concept exists, link to it by its exact title.
If it doesn't exist, create a dangling link anyway (it becomes a stub for later).

## Quality Bar

- Notes must contain **specific details** from the text — quotes, arguments, examples.
  Vague summaries ("this book explores important themes") are worthless.
- Every note must have `sources` pointing to the book's source note title.
- Trust: source note = 7 (you read the full text), concept notes = 5-6.

---

{{note_skill}}
```

- [ ] **Step 3: Verify agent validates**

Run: `cargo run -- agent validate book-import 2>&1`

This requires the workspace to have the agent installed. If the binary isn't built yet
or the agent isn't installed, verify after the next `cargo build`:

Run:
`cargo build && cargo run -- init 2>/dev/null; cargo run -- agent validate book-import`
Expected: `book-import ok`

- [ ] **Step 4: Commit**

```bash
git add assets/agents/book-import/
git commit -m "feat: book-import agent for note creation from imported books"
```

---

## Task 7: Update Reference Import Skill

**Files:**

- Modify: `assets/skills/reference-import/skill.md`

- [ ] **Step 1: Add book import section**

Insert before the `## Files on Disk` section in
`assets/skills/reference-import/skill.md`:

````markdown
## Book Import (EPUB)

### Workflow

1. **Convert** (fast — pure Rust, no external deps):

```json
{ "command": "ghost convert epub --path /path/to/book.epub" }
```
````

The output shows the staging path, title, authors, and chapter count.

2. **Import**:

```json
{
  "command": "ghost reference import /path/to/.staging/book-slug --topic books/<slug> --source-type book"
}
```

Pick a descriptive `--topic` under `books/` (e.g., `books/animal-farm`,
`books/mute-compulsion`).

3. **Create notes** — after import, run the book-import agent to generate structured
   notes from the imported chapters:

```json
{
  "command": "ghost agent run book-import --topic books/<slug> --title '<title>' --authors '<authors>'"
}
```

The agent reads all imported chapters and creates:

- A **source note** summarizing the book (archetype: `source`)
- An **author entity note** (or updates an existing one)
- **Secondary concept/theme notes** linked to the source note and existing knowledge

### Example: Full book import flow

```
ghost convert epub --path ~/Documents/books/Animal\ Farm.epub
# Output: /home/user/GHOST/.staging/animal-farm
#         source_type=book
#         title=Animal Farm
#         authors=George Orwell
#         chapters=11

ghost reference import /home/user/GHOST/.staging/animal-farm \
    --topic books/animal-farm --source-type book

ghost agent run book-import \
    --topic books/animal-farm \
    --title "Animal Farm" \
    --authors "George Orwell"
```

````

- [ ] **Step 2: Update the skill description**

Update the YAML frontmatter `description` to mention books:

```yaml
description:
  Import and update external content in the knowledge base — git repos, web crawls,
  documents (PDF, DOCX), and books (EPUB). Use when the OPERATOR wants persistent,
  searchable reference material from a git repository, website, document, or book,
  or when existing references may be stale and need refreshing.
````

- [ ] **Step 3: Update the CLI Reference section at the bottom**

Add to the CLI Reference block:

```
ghost convert epub --path <path> [--output <dir>]
```

- [ ] **Step 4: Run `just fmt`**

Run: `just fmt` Expected: formats markdown files

- [ ] **Step 5: Commit**

```bash
git add assets/skills/reference-import/skill.md
git commit -m "feat: add book import section to reference-import skill"
```

---

## Task 8: End-to-End Test — Convert + Import

**Files:**

- Create: `tests/epub_import.rs`

This test does NOT require network access or LLM APIs — it's pure Rust (rbook + htmd +
SQLite). No feature flag needed for these steps. The test EPUB is at
`/home/tolki/Documents/books/Animal Farm.epub`.

- [ ] **Step 1: Create `tests/epub_import.rs`**

```rust
mod common;

use std::path::Path;

use ghost::convert::epub::convert_epub;
use ghost::db;
use ghost::reference_import::{ImportProvenance, import_from_path};

/// Test EPUB path — Animal Farm by George Orwell.
///
/// 13 spine items: titlepage, title page, 11 chapters.
/// Some trivial items are filtered, so expect >= 10 chapters.
const TEST_EPUB: &str = "/home/tolki/Documents/books/Animal Farm.epub";

/// Full pipeline: EPUB → staging (per-chapter markdown) → reference import → DB.
#[tokio::test]
async fn epub_convert_and_import() {
    if !Path::new(TEST_EPUB).exists() {
        eprintln!("skipping epub test: {TEST_EPUB} not found");
        return;
    }

    let (db, config, _workspace, _config_dir) = common::test_database().await;
    let workspace_path = Path::new(&config.workspace);

    // --- Phase 1: Convert EPUB to staging ---

    let staging_root = workspace_path.join(".staging");
    let result = convert_epub(&staging_root, Path::new(TEST_EPUB))
        .expect("convert_epub should succeed");

    // Metadata assertions
    assert_eq!(
        result.metadata.title.as_deref(),
        Some("Animal Farm"),
        "title should be 'Animal Farm'"
    );
    assert!(
        result.metadata.authors.iter().any(|a| a.contains("Orwell")),
        "authors should contain Orwell, got: {:?}",
        result.metadata.authors
    );

    // Chapter count: 13 spine items minus trivial ones
    assert!(
        result.chapter_count >= 10,
        "should have >= 10 chapters, got {}",
        result.chapter_count
    );

    // Staging dir should exist with numbered markdown files
    assert!(result.staging_dir.exists(), "staging dir should exist");
    let md_files: Vec<_> = std::fs::read_dir(&result.staging_dir)
        .expect("read staging dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                == Some("md")
        })
        .collect();
    assert_eq!(
        md_files.len(),
        result.chapter_count,
        "markdown file count should match chapter_count"
    );

    // Original EPUB should be preserved
    let originals = result.staging_dir.join("_originals");
    assert!(originals.exists(), "_originals dir should exist");
    let original_files: Vec<_> = std::fs::read_dir(&originals)
        .expect("read originals")
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(original_files.len(), 1, "should have one original file");

    // _metadata.json should exist
    let metadata_path = result.staging_dir.join("_metadata.json");
    assert!(metadata_path.exists(), "_metadata.json should exist");

    // Read a chapter and verify it contains recognizable content
    let mut found_content = false;
    for entry in &md_files {
        let content = std::fs::read_to_string(entry.path()).expect("read chapter");
        if content.contains("Manor Farm") || content.contains("Old Major") {
            found_content = true;
            break;
        }
    }
    assert!(
        found_content,
        "at least one chapter should contain 'Manor Farm' or 'Old Major'"
    );

    // No chapter should start with the book title as first line
    for entry in &md_files {
        let content = std::fs::read_to_string(entry.path()).expect("read chapter");
        let first_line = content.lines().next().unwrap_or("");
        let bare = first_line.trim_start_matches('#').trim();
        assert_ne!(
            bare.to_lowercase(),
            "animal farm",
            "chapter {} should not start with book title",
            entry.file_name().to_string_lossy()
        );
    }

    // --- Phase 2: Import from staging ---

    let topic = "books/animal-farm";
    let provenance = ImportProvenance {
        source_type: Some("book".to_string()),
        source_url: Some(TEST_EPUB.to_string()),
        version_ref: None,
        git_ref: None,
    };

    let import_result = import_from_path(
        &db,
        workspace_path,
        &result.staging_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("import_from_path should succeed");

    assert!(
        import_result.references_created >= 10,
        "should create >= 10 references, got {}",
        import_result.references_created
    );
    assert_eq!(
        import_result.references_skipped, 0,
        "first import should skip nothing"
    );

    // References should exist on disk
    let ref_dir = workspace_path.join("references").join(topic);
    assert!(ref_dir.exists(), "reference dir should exist on disk");

    // References should exist in DB with correct topic
    let db_topic = db::knowledge::find_topic_by_name(&db, topic)
        .await
        .expect("find topic")
        .expect("topic should exist");

    let ref_count = db::knowledge::count_references_by_topic(&db, &db_topic.id)
        .await
        .expect("count refs");
    assert_eq!(
        ref_count as usize, import_result.references_created,
        "DB ref count should match created count"
    );

    // Parent topic "books" should also exist
    let parent = db::knowledge::find_topic_by_name(&db, "books")
        .await
        .expect("find parent")
        .expect("parent topic 'books' should exist");
    assert_ne!(parent.id, db_topic.id);

    // Import batch should exist with source_type = "book"
    let batch = db::knowledge::get_import_batch_by_topic(&db, &db_topic.id)
        .await
        .expect("get batch")
        .expect("import batch should exist");
    assert_eq!(batch.source_type, "book");

    // _import.toml should exist on disk
    let import_toml = ref_dir.join("_import.toml");
    assert!(import_toml.exists(), "_import.toml should exist");
    let toml_content = std::fs::read_to_string(&import_toml).expect("read _import.toml");
    assert!(
        toml_content.contains("source_type = \"book\""),
        "_import.toml should contain source_type = book"
    );

    // Topic index note should exist
    let note_path = workspace_path.join("notes/books/animal-farm/index.md");
    assert!(note_path.exists(), "topic index note should exist");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test epub_convert_and_import -- --nocapture 2>&1` Expected: PASS — all
assertions pass

- [ ] **Step 3: Commit**

```bash
git add tests/epub_import.rs
git commit -m "test: end-to-end EPUB convert + import test with Animal Farm"
```

---

## Task 9: End-to-End Test — Agent Note Creation

**Files:**

- Modify: `tests/epub_import.rs`

This test requires a real LLM API — gated behind `live-tests-llms` feature flag. It
builds on the convert + import from Task 8, then runs the `book-import` agent and
verifies it creates proper notes.

**Important**: This test uses `common::live_test_database()` (not `test_database()`)
because it needs the full `AgentRunner` with real LLM config.

- [ ] **Step 1: Add the agent test function**

Append to `tests/epub_import.rs`:

```rust
/// Full pipeline including agent note creation.
/// Requires LLM API access — gated behind live-tests-llms.
#[cfg(feature = "live-tests-llms")]
#[tokio::test]
async fn epub_agent_creates_notes() {
    use std::collections::HashMap;
    use std::time::Duration;

    if !Path::new(TEST_EPUB).exists() {
        eprintln!("skipping epub agent test: {TEST_EPUB} not found");
        return;
    }

    let env = common::live_test_database("epub_agent").await;
    let workspace_path = Path::new(&env.config.workspace);

    // --- Convert + Import (same as Task 8) ---

    let staging_root = workspace_path.join(".staging");
    let convert_result = convert_epub(&staging_root, Path::new(TEST_EPUB))
        .expect("convert_epub");

    let topic = "books/animal-farm";
    let provenance = ImportProvenance {
        source_type: Some("book".to_string()),
        source_url: Some(TEST_EPUB.to_string()),
        version_ref: None,
        git_ref: None,
    };

    let import_result = import_from_path(
        &env.db,
        workspace_path,
        &convert_result.staging_dir,
        topic,
        &provenance,
        None,
    )
    .await
    .expect("import_from_path");

    env.log(format!(
        "imported {} references for {topic}",
        import_result.references_created
    ));

    // --- Run book-import agent ---

    let mut args = HashMap::new();
    args.insert("topic".into(), topic.into());
    args.insert("title".into(), "Animal Farm".into());
    args.insert("authors".into(), "George Orwell".into());

    let agent_result = tokio::time::timeout(
        Duration::from_secs(300),
        env.agent_runner.run_with_args("book-import", args, None),
    )
    .await
    .expect("agent should complete within 5 minutes")
    .expect("agent should succeed");

    env.log(format!("agent session: {}", agent_result.session_id));
    if let Some(ref findings) = agent_result.findings {
        env.log(format!("findings: {findings}"));
    }

    // --- Verify notes were created ---

    // Search for a source note about Animal Farm
    let notes = db::knowledge::search_notes(&env.db, "Animal Farm", 10)
        .await
        .expect("search notes");

    let source_note = notes.iter().find(|n| n.archetype.as_deref() == Some("source"));
    assert!(
        source_note.is_some(),
        "should have a source note about Animal Farm. Found notes: {:?}",
        notes.iter().map(|n| &n.title).collect::<Vec<_>>()
    );

    let source_note = source_note.expect("source note");
    let source_body = &source_note.body;

    // Source note should mention key themes/elements
    let has_thematic_content = source_body.contains("allegory")
        || source_body.contains("satire")
        || source_body.contains("totalitarian")
        || source_body.contains("revolution")
        || source_body.contains("power")
        || source_body.contains("corruption");
    assert!(
        has_thematic_content,
        "source note should mention themes (allegory/satire/totalitarianism/revolution/power/corruption)"
    );

    // Search for an author note about George Orwell
    let author_notes = db::knowledge::search_notes(&env.db, "George Orwell", 10)
        .await
        .expect("search author notes");

    let author_note = author_notes
        .iter()
        .find(|n| n.archetype.as_deref() == Some("entity") && n.title.contains("Orwell"));
    assert!(
        author_note.is_some(),
        "should have an entity note about George Orwell. Found: {:?}",
        author_notes.iter().map(|n| &n.title).collect::<Vec<_>>()
    );

    // Author note should link to Animal Farm
    let author_body = &author_note.expect("author note").body;
    assert!(
        author_body.contains("Animal Farm"),
        "author note should mention Animal Farm"
    );
}
```

- [ ] **Step 2: Run the test (requires LLM API)**

Run:
`cargo test epub_agent_creates_notes --features live-tests-llms -- --nocapture 2>&1`
Expected: PASS — agent creates source note + author note with proper content

If the test fails due to agent behavior (not code bugs), examine the agent session:

```bash
cargo run -- agent status --agent book-import
cargo run -- agent show <run-id> --full
```

- [ ] **Step 3: Commit**

```bash
git add tests/epub_import.rs
git commit -m "test: book-import agent live test with Animal Farm"
```

---

## Task 10: Final Verification — Full E2E + CI

Everything is implemented. Now re-run both tests back-to-back to catch any regressions
from later tasks (e.g., import type changes breaking the convert test, agent files not
bundling correctly).

- [ ] **Step 1: Run `just ci`**

Run: `just ci` Expected: all checks pass, zero clippy warnings, all unit tests pass

- [ ] **Step 2: Re-run the convert + import e2e test**

Run: `cargo test epub_convert_and_import -- --nocapture 2>&1` Expected: PASS — validates
that Tasks 4-5 (migration + type changes) didn't break the convert/import pipeline

- [ ] **Step 3: Re-run the agent e2e test**

Run:
`cargo test epub_agent_creates_notes --features live-tests-llms -- --nocapture 2>&1`
Expected: PASS — validates the full pipeline end-to-end: convert → import → agent →
notes (source note + author note)

If the agent test fails, inspect the run:

```bash
cargo run -- agent status --agent book-import
cargo run -- agent show <run-id> --full
```

- [ ] **Step 4: Manual smoke test via CLI**

```bash
cargo run -- convert epub --path "/home/tolki/Documents/books/Animal Farm.epub"
# Should print staging path, title=Animal Farm, authors=George Orwell, chapters=11

cargo run -- reference import <staging-path> --topic books/animal-farm-smoke --source-type book
# Should import all chapters, print progress

cargo run -- topics list
# Should show books/animal-farm-smoke with correct ref count

# Cleanup
cargo run -- reference delete --topic books/animal-farm-smoke
```

- [ ] **Step 5: Final commit (if any fixups were needed)**

```bash
git add -A
git commit -m "fix: final adjustments from e2e verification"
```
