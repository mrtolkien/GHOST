# EPUB Book Import

Import EPUB books as chapter-level references, then generate structured notes — a source
note summarizing the book and secondary notes for key concepts, themes, and the author —
making the knowledge surfaceable to the GHOST.

## Overview

A new source type for the existing convert → import pipeline:

```
ghost convert epub --path ./book.epub     →  staging dir with per-chapter markdown
ghost reference import <staging> --topic books/animal-farm --source-type book
```

After import, the GHOST runs the `book-import` agent to produce notes.

## Implementation Phases

### Phase 1: EPUB Conversion + Import (this spec)

Everything below is scoped to this phase.

### Phase 2: Optional Nix Dependencies

Rework the flake to support optional heavy dependencies without installing them by
default. See `backlog/tasks/9-extras/optional-nix-dependencies.md`.

### Phase 3: Podcast and Video Import

See `backlog/tasks/9-extras/audio-content-import.md`.

---

## Phase 1 Spec

### CLI: `ghost convert epub`

New variant in `ConvertCommand` (`src/cli/convert.rs`):

```
ghost convert epub --path ./book.epub [--output <dir>]
```

- `--path`: Path to the EPUB file (required)
- `--output`: Staging directory override (default: `<workspace>/.staging`)

Follows the same two-step pattern as PDF/git/crawl: convert produces a staging
directory, then `ghost reference import` indexes it.

### CLI: `ghost reference import` (existing, unchanged)

```
ghost reference import <staging-dir> --topic books/animal-farm --source-type book
```

The existing import command handles book imports unchanged — it already walks a
directory of markdown files and creates references. The only change is accepting `book`
as a `--source-type` value.

### Conversion: EPUB → Markdown (Rust-native)

**No external binary dependencies.** Use two Rust crates:

- **`rbook`** (0.7.5, Apache-2.0) — EPUB parser with spine-based chapter iteration
- **`htmd`** (0.5.3, Apache-2.0) — already a dependency, used for web content extraction

Validated against two real EPUBs with very different structures:

- _Animal Farm_ (well-structured, proper H1/H2 headings) — clean chapter splits
- _Mute Compulsion_ (no markdown headings, pure CSS-styled divs) — still splits cleanly
  on spine items

#### New Module: `src/convert/epub.rs`

Follows the same pattern as `pdf.rs`:

```rust
pub struct EpubConvertResult {
    pub staging_dir: PathBuf,
    pub chapter_count: usize,
    pub metadata: EpubMetadata,
}

pub struct EpubMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub publication_date: Option<String>,
}

pub fn convert_epub(
    staging_root: &Path,
    epub_path: &Path,
) -> Result<EpubConvertResult, ConvertError>
```

This is a synchronous function (no async needed — pure file I/O, no network).

#### Conversion Pipeline

```
EPUB file
  → rbook::Epub::open(path)
  → Extract metadata (title, author, language, publisher, date)
  → Iterate spine items in reading order
  → For each spine item:
      → htmd converts XHTML → markdown (skip script/style/svg tags)
      → Skip trivial items (< 20 bytes after trim)
      → Strip book title from first line (htmd includes <title> tag content)
      → Write as markdown file in staging dir
  → Copy original EPUB to staging/_originals/
  → Write _metadata.json with EpubMetadata (for the import step)
```

#### Output Structure (staging dir)

```
.staging/
  animal-farm/
    _originals/
      Animal Farm.epub
    _metadata.json            # EpubMetadata serialized
    00-title.md
    01-chapter-i.md
    02-chapter-ii.md
    ...
```

#### File Naming

Derive clean filenames from spine item hrefs. Strip garbled epub IDs, use sequential
numbering + sanitized name. If the spine item name is unusable, fall back to
`{idx:02}-chapter-{idx}.md`.

Use `staging::slug_from_source()` for the staging directory name (it already handles
file paths — extracts file stem).

#### stdout Output

Print metadata to stdout following the existing pattern for other converters:

```
/path/to/.staging/animal-farm
source_type=book
title=Animal Farm
authors=George Orwell
chapters=11
```

### Database: New `book` Source Type

#### Migration

Add `'book'` to the `import_batch.source_type` CHECK constraint:

```sql
-- New migration: XXX_book_source_type.sql
-- SQLite doesn't support ALTER CHECK, so recreate the constraint
-- via the standard alter-table-rename dance
```

#### `ImportConfigJson` Extension

Add optional book metadata fields to `ImportConfigJson` in
`src/reference_import/types.rs`:

```rust
// Add to ImportConfigJson:
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

These fields are `Option` and `skip_serializing_if` so they don't affect existing
git/crawl/file imports at all.

#### `ImportSource` Extension

Add a `Book` variant to `ImportSource`:

```rust
Book {
    path: String,
    title: Option<String>,
    authors: Vec<String>,
}
```

#### `_import.toml` for Books

```toml
# Auto-generated by ghost reference import
source_type = "book"
source_url = "/home/user/Documents/books/Animal Farm.epub"
title = "Animal Farm"
authors = ["George Orwell"]
language = "en"
ref_count = 11
```

### Import Flow: Metadata Passthrough

The convert step writes `_metadata.json` to the staging dir. The import step reads it:

1. `ghost convert epub --path ./book.epub` → staging dir + `_metadata.json`
2. `ghost reference import <staging> --topic books/animal-farm --source-type book`
   - Import detects `_metadata.json` in the staging dir
   - Reads book metadata from it
   - Passes metadata through to `ImportConfigJson` and `_import.toml`

Alternatively, the GHOST reads the convert stdout + staging dir to construct the full
`ghost reference import` command with appropriate flags. The metadata flows through the
existing provenance system.

### Note-Writing: `book-import` Agent

After import, the GHOST runs the `book-import` agent via the existing agent system. The
reference-import skill should instruct the GHOST to spawn this agent after a book
import.

#### Agent: `assets/agents/book-import/`

```
assets/agents/book-import/
  agent.lua
  prompt.md
```

#### `agent.lua`

```lua
local template = require("ghost.template")

return {
    name = "book-import",
    description = "Create structured notes from imported book references",
    max_iterations = 40,

    tools = {
        "file_read",
        "knowledge_search",
        "note_write",
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

        local user_message = "Import book as notes.\n\n"
            .. "**Title**: " .. title .. "\n"
            .. "**Author(s)**: " .. authors .. "\n"
            .. "**Topic**: " .. topic .. "\n\n"
            .. "Read the chapters at `references/" .. topic .. "/` and create notes."

        return {
            system_prompt = system_prompt,
            messages = { { role = "user", content = user_message } },
        }
    end,
}
```

#### `prompt.md` — Key Instructions

The agent prompt must cover:

**For all books:**

- Read ALL imported chapters to understand the full work
- Create a **core source note** (`archetype: source`) for the book itself:
  - Title, author, publication context
  - Central thesis / narrative arc
  - Structure overview (what each part covers)
  - This note can be longer than typical notes (up to ~800 words)
  - Link to reference files is NOT needed (references are managed automatically)
- Create an **author entity note** (`archetype: entity`) if one doesn't already exist:
  - Key biographical facts relevant to understanding their work
  - Other notable works
  - Link back to the source note: `[[wrote>Book Title]]`
- Create **secondary concept notes** for major ideas/themes:
  - Each is a standalone `entity` or `analysis` note
  - Links back to the source note via `[[from>Book Title]]`
  - Links to the author via `[[by>Author Name]]`
  - Links to any existing notes about related concepts
- **Search before creating**: always `knowledge_search` for existing notes about
  concepts, the author, related topics. Update existing notes rather than duplicating.
  Link to them.
- Notes should read like Wikipedia articles — structured, factual, linked

**For non-fiction books:**

- Focus on capturing the **logic and argumentation**: what is the author's thesis, what
  evidence do they present, how do they build their case
- Concept notes should capture the book's key arguments and frameworks
- Use `analysis` archetype for notes about the author's reasoning frameworks

**For fiction books:**

- Focus on extracting **themes** — the big ideas the work explores
- Concept notes cover themes (power, corruption, freedom), not plot summaries
- Character notes only if a character embodies a theme worth referencing independently
- Use `entity` archetype for theme notes

**Linking strategy (critical):**

- The agent MUST search existing notes and link to them. Think like Wikipedia:
  - Does a note about this historical period already exist? Link it.
  - Does a note about this philosophical concept exist? Link it.
  - Is the author already in the knowledge base? Update that note.
- Use typed edges: `[[about>Theme]]`, `[[by>Author]]`, `[[from>Book Title]]`,
  `[[compares>Other Work]]`, `[[influenced_by>Earlier Work]]`

### Skill Update: `reference-import`

Add a book import section to `assets/skills/reference-import/skill.md`:

````markdown
## Book Import (EPUB)

### Workflow

1. **Convert**:
   ```json
   { "command": "ghost convert epub --path /path/to/book.epub", "background": false }
   ```
````

EPUB conversion is fast (pure Rust, no network) — no need for background mode.

2. **Import**:

   ```json
   {
     "command": "ghost reference import /path/to/.staging/book-slug --topic books/<slug> --source-type book"
   }
   ```

3. **Note creation**: After import, run the book-import agent:
   ```json
   {
     "command": "ghost agent run book-import --topic books/<slug> --title '<title>' --authors '<authors>'"
   }
   ```

The agent reads the imported chapters and creates:

- A source note summarizing the book
- An author entity note (or updates an existing one)
- Secondary concept/theme notes linked to the source note and existing knowledge

````

### Integration with Existing Systems

- References stored via existing `import_from_path()` flow (upsert on topic_id + path)
- Import batch tracks `source_type = 'book'`, with book metadata in `import_config` JSON
- File watcher auto-triggers embedding pipeline (chunk → Ollama → sqlite-vec)
- FTS5 indexes chapter content automatically via existing triggers
- Notes created by the agent via existing `note_write` tool with proper archetypes
- Wiki links in notes create graph edges via `reconcile_edges()`

### Quality Observations from Testing

**rbook+htmd output quality** is essentially identical to pandoc for prose-heavy books.
Both produce clean markdown with proper emphasis, blockquotes, footnote links, and
paragraph structure. Minor differences:

- htmd includes the `<title>` tag content as the first line of each chapter (strip it)
- Internal epub cross-reference links are preserved as ugly anchors (harmless noise for
  search/embedding, can be stripped in post-processing)
- Tables are handled adequately by htmd for simple cases

**Spine-based splitting** is more reliable than heading-based splitting. EPUBs with no
markdown headings (like _Mute Compulsion_) still split correctly because the spine
structure is the epub's canonical chapter boundary — it's what e-readers use.

### Crate Dependencies

Add to `Cargo.toml`:

```toml
rbook = "0.7"  # EPUB parser (Apache-2.0)
# htmd already present (0.5)
````

### Files to Create/Modify

| File                                      | Action | Description                                                     |
| ----------------------------------------- | ------ | --------------------------------------------------------------- |
| `Cargo.toml`                              | modify | Add `rbook` dependency                                          |
| `src/convert/epub.rs`                     | create | EPUB → markdown converter                                       |
| `src/convert/mod.rs`                      | modify | Add `pub mod epub;`                                             |
| `src/convert/error.rs`                    | modify | Add `Epub(String)` variant if needed                            |
| `src/cli/convert.rs`                      | modify | Add `Epub` variant to `ConvertCommand`                          |
| `src/reference_import/types.rs`           | modify | Add `Book` to `ImportSource`, book fields to `ImportConfigJson` |
| `src/cli/reference.rs`                    | modify | Accept `book` in `--source-type`                                |
| `migrations/XXX_book_source_type.sql`     | create | Add `'book'` to CHECK constraint                                |
| `assets/agents/book-import/agent.lua`     | create | Agent definition                                                |
| `assets/agents/book-import/prompt.md`     | create | Agent prompt                                                    |
| `assets/skills/reference-import/skill.md` | modify | Add book import section                                         |
| `tests/epub_import.rs`                    | create | End-to-end test                                                 |

---

## End-to-End Test

One integration test covering the full pipeline. Uses the real _Animal Farm_ EPUB at
`/home/tolki/Documents/books/Animal Farm.epub`.

### Test File: `tests/epub_import.rs`

Test fixture copies the EPUB to a temp workspace to avoid depending on the user's
filesystem in CI — but for now (pre-alpha, single developer), pointing at the real file
is acceptable.

### Test Steps

1. **Convert**: Call `convert_epub(staging_root, epub_path)` directly (no CLI
   subprocess)
   - Assert `EpubConvertResult` has `chapter_count >= 10` (Animal Farm has 13 spine
     items: titlepage + title + 11 chapters; trivial-content filter may drop 1-2)
   - Assert `metadata.title` is `Some("Animal Farm")`
   - Assert `metadata.authors` contains `"George Orwell"` (or similar)
   - Assert staging dir exists and contains numbered `.md` files
   - Assert `_originals/Animal Farm.epub` exists in staging dir
   - Read one chapter file — assert it contains recognizable Animal Farm text (e.g.
     `"Manor Farm"` or `"Old Major"`)
   - Assert no chapter file starts with the book title as first line (title stripping)

2. **Import**: Call `import_from_path()` with the staging dir
   - Assert `references_created >= 10`
   - Assert `references_skipped == 0`
   - Assert reference files exist on disk at `references/books/animal-farm/`
   - Assert references exist in DB with correct topic
   - Assert `_import.toml` exists with `source_type = "book"`

3. **Agent note creation**: Run the `book-import` agent against the imported references
   - Assert at least one note exists with `archetype = "source"` and title containing
     "Animal Farm"
   - Assert the source note body contains key terms: mentions of allegory/satire/
     totalitarianism/revolution (at least some subset)
   - Assert at least one note exists about "George Orwell" (the author) with
     `archetype = "entity"`
   - Assert the author note links to the book source note
   - Assert wiki links in the source note resolve to real notes or references
   - This step requires `live-tests-llms` feature flag (calls a real LLM)

### Feature Flags

- Steps 1-2: no feature flag needed (pure Rust, SQLite)
- Step 3: `live-tests-llms` (requires LLM API access for agent execution)
