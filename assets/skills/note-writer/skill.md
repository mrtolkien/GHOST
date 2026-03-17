---
name: note-writer
description:
  Read before creating or updating knowledge notes with note_write. Covers mandatory
  conventions — truth principle, archetypes, titles, typed linking, tags, trust scores,
  evidence bar, consolidation, and archival. Skip for diary entries and simple file
  writes.
---

# Note Writer — Knowledge Note Guide

This skill is the complete reference for writing structured knowledge notes. Follow
every section when creating or updating notes.

## Truth Principle

**Notes are TRUE information.** They are facts the OPERATOR can trust without
re-verifying. Do not create a note unless the information is:

- **Verified through a source you actually read** — a `web_fetch`'d page, a
  `file_read`'d reference, or official documentation.
- **Directly stated by the OPERATOR** — use `message:{ULID}` in `sources` to cite the
  message.
- **Derived from other TRUE notes** — link to them via `[[based_on>Note Title]]` and
  explain your reasoning in the body. Derived notes start at trust 4.

If you cannot point to evidence for a claim, do not save it as a note. Speculation
belongs in the diary as an open question, not in the knowledge base.

## Before You Write

Always discover what already exists before creating anything:

1. **Search**: `knowledge_search` for the topic — check for existing notes to update
   rather than duplicate.
2. **Tags**: `ghost knowledge tags` via `shell` — see existing tag hierarchies before
   inventing new ones.
3. **Graph**: `ghost knowledge graph "Title"` — check how related notes connect.

**Update existing notes rather than creating duplicates.** If a note covers the same
entity, extend it with new information instead of writing a second note.

## What Deserves a Note

Create a note when information is:

- **Specific and reusable** — concrete facts, data, or structured reasoning you'd want
  to find again.
- **Stable enough to reference** — not a fleeting thought or in-progress speculation.
- **Worth preserving locally** — saves a future web fetch, captures data that could
  disappear, or records knowledge not easily re-found.

## Consolidation over Fragmentation

When several items share a parent concept, create one consolidated note rather than
individual notes per item. A single "Slay the Spire 2 Characters" note with a summary of
all characters is more useful than five separate character stubs. Link to references for
full details.

Save detailed individual notes only when an item is central to the OPERATOR's interests
or a decision being made. The test: would the OPERATOR search for this specific item by
name?

## What Does NOT Deserve a Note

- **Transient status updates** — "tried X, didn't work yet" belongs in the diary.
- **Vague impressions** — without concrete facts or actionable detail.
- **Information already captured** — in an existing note. Update it instead.
- **Items mentioned only in passing** — if it wasn't central to a decision or the
  OPERATOR's interests, it doesn't need its own note.
- **Unverified analysis** — if you haven't read a source confirming it, it's not a fact.
  Either find evidence or don't create the note.

## Archetypes

Every note MUST have an archetype. Choose the one that best fits:

| Archetype  | Purpose                                              | Default trust |
| ---------- | ---------------------------------------------------- | ------------- |
| `entity`   | Factual description of a thing                       | 5             |
| `analysis` | Reasoning framework and trade-offs (NOT conclusions) | 4             |
| `source`   | Evaluation of an information source                  | 5             |
| `profile`  | Information about the OPERATOR                       | 6             |
| `topic`    | Topic hub — navigation and discovery                 | 5             |

### Archetype examples

| Note title                      | Archetype  | Why                                            |
| ------------------------------- | ---------- | ---------------------------------------------- |
| "GeForce RTX 4090"              | `entity`   | Factual description of a product               |
| "LLM Hardware Trade-offs"       | `analysis` | Compares options, captures reasoning           |
| "Tom's Hardware — 3D Printing"  | `source`   | Evaluates reliability of an information source |
| "OPERATOR Skincare Preferences" | `profile`  | Personal information about the OPERATOR        |
| "3D Printing"                   | `topic`    | Topic hub connecting subtopics                 |
| "Pinchflat"                     | `entity`   | Description of a software tool                 |
| "Niacinamide"                   | `entity`   | Description of a skincare ingredient           |
| "Self-hosted YouTube Stack"     | `analysis` | Trade-off analysis with structured reasoning   |

### Archetype rules

- **`entity`** is the default. Use it for products, tools, concepts, people,
  organizations — anything that is primarily a factual description.
- **`analysis`** captures your reasoning framework and trade-offs, NOT your conclusions.
  Conclusions belong in the diary. Analysis notes MUST link to evidence via
  `[[based_on>...]]` or `[[compares>...]]` edges. When the OPERATOR revisits a topic,
  re-derive recommendations from evidence notes rather than restating prior analysis.
- **`source`** evaluates a source's reliability and depth. Tag under `{domain}/sources`.
  Title format: "Source Name — Topic". MUST link with `[[about>Topic]]`.
- **`profile`** is ONLY for information about the OPERATOR. Other people are entities.
  MUST be tagged `operator/*` (e.g. `operator/health`, `operator/interests`).
- **`topic`** is for navigation hubs. Auto-created skeleton topic notes MUST be fleshed
  out with a meaningful description — semantic search relies on this.

## Note Guidelines

- **Atomic**: one concept per note, 100-400 words typical.
- **Specific**: exact names, numbers, versions, dates — never vague.
- **Linked**: always use typed edges `[[rel>Title]]` — never raw `[[Title]]`. Typed
  edges make the graph navigable; untyped edges are noise.
- **Tagged**: first tag = subfolder path, normally at least `topic/collection` depth
  (e.g. `rust/async`, `cooking/techniques`). Root-level tags (e.g. `rust`) are allowed
  when the note genuinely describes the topic itself rather than a subtopic within it.
  Max depth is 3 levels (`topic/collection/subcollection`).
- **Trust**: set according to archetype default, then adjust with evidence.

## Titles

Follow Wikipedia naming conventions:

- **Short noun phrases**: "Tokio", not "The Tokio Async Runtime for Rust"
- **No prefixes**: "Async Runtime Comparison", not "Decision: Async Runtime"
- **Proper nouns as-is**: "Visual Studio Code", "Tom's Hardware"
- **Source notes — add topic**: "Source Name — Topic" when the source covers many
  domains (e.g. "Docs.rs — Async Ecosystem")

Consistent titles prevent duplicates and make wiki links predictable.

## Linking (critical)

**Prefer typed edges** — use `[[rel>Title]]` over bare `[[Title]]` whenever a natural
relationship label exists. Typed edges make the knowledge graph navigable: you can
traverse by relationship kind (e.g. "show me everything this entity `uses`"). Bare
`[[Title]]` links are acceptable when no clear relationship label fits, but they should
be the exception.

Every note MUST contain at least one link. If you mention another entity by name, link
it — even if that entity doesn't have a note yet. Dangling links are fine; they create
stubs that get filled in over time.

Common patterns:

- Entity notes → `[[created_by>Org Name]]`, `[[uses>Library]]`, `[[about>Topic]]`
- Analysis notes → `[[compares>Option A]]`, `[[based_on>Evidence Note]]`
- Source quality notes → `[[about>Topic]]`
- Child notes → `[[parent>Parent Entity]]` (set via `parent` frontmatter field)

## Parent Hierarchy

The `parent` frontmatter field creates structural containment. Use it when there is a
clear hierarchical relationship:

- Product → parent: manufacturer ("RTX 4090" → parent: "Nvidia")
- Sub-topic → parent: topic ("Async Runtimes" → parent: "Rust")
- Analysis → parent: topic note

Do NOT force a parent on every note. Only use it when the containment relationship is
obvious and useful for navigation.

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4**: Default for `analysis` notes (your own reasoning, not external facts)
- **5**: Default for most notes. Reasonable confidence.
- **6**: Default for `profile` notes. Based on OPERATOR statements.
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by OPERATOR or primary sources

## Evidence Bar

Every note MUST have at least one entry in its `sources` parameter:

- `https://...` — a web page you `web_fetch`'d (must exist as a reference file)
- `message:{ULID}` — an OPERATOR statement from a specific message

Notes with no sources are rejected. If you're deriving from other notes, you still need
a source — the evidence that the underlying notes are based on.

**Verifying sources**: Your sources are stored as reference files under
`references/{note-topic}/{domain-slug}/`. To re-read a source after the session, use
`file_read` on the matching reference file.

## The `operator` Tag

The `operator` root tag is reserved for information **about the OPERATOR** that
supplements `OPERATOR.md`. Use it for personal facts, preferences, and context that:

- Are TRUE and stable (not fleeting opinions)
- Would help you serve the OPERATOR better in future conversations
- Don't belong in OPERATOR.md (which is core identity, always loaded)

Examples: `operator/health`, `operator/interests`, `operator/languages`.

Use `knowledge_search(archetype="profile")` to discover OPERATOR preferences when you
are unsure about their context on a topic.

## Tags

Use consistent, lowercase, hierarchical tags separated by slashes. Use
`ghost knowledge tags` to see what exists before inventing new hierarchies. Prefer
existing tags over creating new ones.

## Archiving

When you encounter a note whose core claim is provably false, superseded by a newer
note, or past its useful window (time-sensitive data that has expired), archive it using
`note_write` with `action: "archive"`. This moves the note to `notes/.archive/` and
removes it from the active index while preserving its content for reference.

## Rules

- Update existing notes over creating duplicates.
- Notes under ~1500 characters index as a single embedding vector — keep concise.
- Pass source URLs in the `sources` parameter of `note_write` — they will be preserved
  in structured frontmatter. Do NOT put bare URLs in the note body.
- Do NOT use `[[references/...]]` wiki links — references are managed automatically
  after your session.
- Link every entity mentioned — prefer typed edges (`[[rel>Title]]`) to build a
  navigable knowledge graph. Dangling links are fine.
- Every note requires an `archetype` and a `written_at` timestamp (set automatically).
