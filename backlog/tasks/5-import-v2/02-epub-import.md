# Media Import: Books, Podcasts, and Videos

Import books, podcast episodes, and video transcripts as references, then generate
structured notes about them — making the knowledge surfaceable to the GHOST.

## Overview

Three new source types for `ghost reference import`:

1. **Books** (`ghost reference import book`) — EPUB first, MOBI/PDF later
2. **Podcasts** (`ghost reference import podcast`) — transcripts from web/RSS/STT
3. **Videos** (`ghost reference import video`) — YouTube transcripts via subtitles/STT

Each follows a two-phase flow:

1. **Import**: Convert source to markdown references split by chapter/section
2. **Note-writing**: GHOST creates structured notes about the imported source

## Implementation Phases

### Phase 1: EPUB Book Import (implement first)

Everything below is scoped to this phase. Podcasts and videos come later.

### Phase 2: Optional Nix Dependencies

Rework the flake to support optional heavy dependencies without installing them by
default. See `backlog/tasks/9-extras/optional-nix-dependencies.md`.

### Phase 3: Podcast and Video Import

See `backlog/tasks/9-extras/audio-content-import.md`.

---

## Phase 1 Spec: EPUB Book Import

### CLI Command

```
ghost reference import book --path ./book.epub --topic books/animal-farm
```

- `--path`: Path to the EPUB file (required)
- `--topic`: Topic hierarchy for references (required)

### Conversion: EPUB → Markdown (Rust-native)

**No external binary dependencies.** Use two Rust crates:

- **`rbook`** (0.7.5, Apache-2.0) — EPUB parser with spine-based chapter iteration
- **`htmd`** (0.5.3, Apache-2.0) — HTML-to-markdown converter (turndown.js-inspired)

Validated against two real EPUBs with very different structures:

- _Animal Farm_ (well-structured, proper H1/H2 headings) — clean chapter splits
- _Mute Compulsion_ (no markdown headings, pure CSS-styled divs) — still splits cleanly
  on spine items

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
      → Write as reference file
```

#### Output Structure

```
references/
  books/
    animal-farm/
      _originals/
        Animal Farm.epub          # original file preserved
      00-title.md
      01-chapter-i.md
      02-chapter-ii.md
      ...
```

#### File Naming

Derive clean filenames from spine item hrefs. Strip garbled epub IDs, use sequential
numbering + sanitized name. If the spine item name is unusable, fall back to
`{idx:02}-chapter-{idx}.md`.

#### Metadata

Extract from EPUB and store in the import batch config (like existing import sources):

- Title
- Author(s)
- Language
- Publisher
- Publication date

### Note-Writing Flow

After import, the GHOST asks the user which mode to use:

#### Mode A — Autonomous

For sources the user has **not yet read**. The GHOST spawns an agent that:

1. Reads the imported reference chapters
2. Creates a **core `source` note** summarizing the book (title, author, thesis, key
   arguments, structure). This note is allowed to be longer than typical notes. It links
   to the reference files via wiki links.
3. Creates **secondary concept notes** for major ideas/concepts in the source, each
   pointing back to the core source note.
4. Reports back when done.

The user can later refine notes after reading the book.

#### Mode B — Guided

For sources the user has **already read**. The GHOST spawns an agent that:

1. Reads the imported reference chapters
2. Returns a **proposal**: list of notes with titles, archetypes, and key points
3. The GHOST presents this proposal to the user in chat
4. User approves, edits, or extends the proposal
5. The GHOST continues the agent with the approved list
6. Agent creates the approved notes

### Integration with Existing Systems

- References stored via existing `create_reference()` flow (upsert on topic_id + path)
- Import batch tracks source_type = `book`, with metadata in `import_config` JSON
- File watcher auto-triggers embedding pipeline (chunk → Ollama → sqlite-vec)
- FTS5 indexes chapter content automatically via existing triggers
- Notes created via existing `create_note()` with `source` archetype
- Wiki links in notes create graph edges to other notes via `reconcile_edges()`
- `cited` edges link source notes to reference files

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
