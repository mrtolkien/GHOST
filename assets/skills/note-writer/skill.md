---
name: note-writer
description:
  Read before creating or updating knowledge notes with note_write. Covers mandatory
  conventions — titles, typed linking, tags, trust scores, special note types. Skip for
  diary entries and simple file writes.
---

# Note Writer — Knowledge Note Guide

This skill is the complete reference for writing structured knowledge notes. Follow
every section when creating or updating notes.

## Before You Write

Always discover what already exists before creating anything:

1. **Search**: `knowledge_search` for the topic — check for existing notes to update
   rather than duplicate.
2. **Tags**: `ghost knowledge tags` via `run_shell_command` — see existing tag
   hierarchies before inventing new ones.
3. **Graph**: `ghost knowledge graph "Title"` — check how related notes connect.

**Update existing notes rather than creating duplicates.** If a note covers the same
entity, extend it with new information instead of writing a second note.

## What Deserves a Note

Create a note when information is:

- **Specific and reusable** — concrete facts, decisions, or insights you'd want to find
  again (names, versions, trade-offs, how-tos).
- **Stable enough to reference** — not a fleeting thought or in-progress speculation.

Skip notes for:

- Transient status updates ("tried X, didn't work yet")
- Vague impressions without actionable detail
- Information already captured in an existing note

When in doubt, err toward creating — a short, specific note is better than lost
knowledge. But a vague note is worse than no note.

## Note Guidelines

- **Atomic**: one concept per note, 100-400 words typical.
- **Specific**: exact names, numbers, versions, dates — never vague.
- **Linked**: always use typed edges `[[rel>Title]]` — never raw `[[Title]]`. Typed
  edges make the graph navigable; untyped edges are noise.
- **Tagged**: first tag = subfolder path, normally at least `topic/collection` depth
  (e.g. `rust/async`, `cooking/techniques`). Root-level tags (e.g. `rust`) are allowed
  when the note genuinely describes the topic itself rather than a subtopic within it.
  Max depth is 3 levels (`topic/collection/subcollection`).
- **Trust**: start at 5, raise with evidence, lower for speculation (1-10).

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

Every entity note MUST contain at least one link. If you mention another entity by name,
link it — even if that entity doesn't have a note yet. Dangling links are fine; they
create stubs that get filled in over time. Don't avoid linking just because the target
note doesn't exist.

Common patterns:

- Entity notes → `[[created_by>Org Name]]`, `[[uses>Library]]`
- Comparison notes → `[[compares>Option A]]` vs `[[compares>Option B]]`
- Source quality notes → `[[about>Topic]]`
- Entity under a topic → `[[about>Topic Name]]` (makes topic notes natural graph hubs)

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4-6**: Reasonable confidence, based on experience or documentation
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by OPERATOR or primary sources

Start at 5 for most notes. Adjust as confidence changes.

## Tags

Use consistent, lowercase, hierarchical tags separated by slashes. Use
`ghost knowledge tags` to see what exists before inventing new hierarchies. Prefer
existing tags over creating new ones.

## Special Note Types

**Source quality notes**: When you've consulted external sources, rate at least one
source's reliability and depth. Tag under `{domain}/sources`. Title format: "Source Name
— Topic" (the same site may vary in quality across domains). Link with
`[[about>Topic]]`.

**Decision notes**: When comparisons or trade-offs are involved, create a decision note
linking the options with rationale. Use `[[compares>Option A]]` /
`[[compares>Option B]]` edges.

## Rules

- Update existing notes over creating duplicates.
- Notes under ~1500 characters index as a single embedding vector — keep concise.
- Pass source URLs in the `sources` parameter of `note_write` — they will be preserved
  in structured frontmatter. Do NOT put bare URLs in the note body.
- Do NOT use `[[references/...]]` wiki links — references are managed automatically
  after your session.
- Link every entity mentioned — prefer typed edges (`[[rel>Title]]`) to build a
  navigable knowledge graph. Dangling links are fine.
