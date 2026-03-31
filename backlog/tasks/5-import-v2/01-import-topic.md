# Unified Reference Topic Assignment

## Problem

Reference topic assignment happens in two places:

1. **Import flow** (GHOST-driven): GHOST converts source to staging, reads the content,
   picks `--topic`. The two-step convert + import design (see `00-split-import.md`)
   solved the old "must know topic before reading content" problem. Remaining gap: no
   shared conventions for how to pick a good topic name.

2. **Curation flow** (code-driven, post-reflection): moves cited web cache files into
   `references/`. Topic comes from the note that cites the URL via its first tag. This
   is the correct design — the GHOST already did the topic assignment by putting the
   citation in a note with the right tags.

### Curation stores cited-but-unreferenced pages as domain-slug orphans

Curation decides a cache file is "used" if it was `cited` (URL appears in the agent's
`## Sources` output) OR if its URL appears in a note's `sources:` field. When a note
references it, curation gets the topic from the note's first tag — this works correctly.

But `cited` alone (no note match) also triggers storage, using `topic_from_url()` which
produces domain slugs like `btod-com`, `ikea-com`, `timeout-jp`. These references have
no note pointing to them and end up as orphan topics.

Logfire data (March 25-31) confirms: the majority of curated moves land in domain-slug
topics. Note-scoped moves (`llm/cerebras-ai`, `ghost/github-com`) are the minority —
only when a note with the URL in `sources:` existed at curation time.

Evidence from production:

- `btod-com/`, `ikea-com/`, `flexispot-com/`, `product-okamura-co-jp/` — all standing
  desk pages. The GHOST cited them in findings but the reflection agent wrote notes that
  reference the on-disk paths (after a manual `cp` on March 24), not the URLs.
- `dtxmania-net/`, `keio-co-jp/`, `timeout-jp/`, `mapfan-com/`, `cdc-gov/` — all
  domain-slug orphans from curation's `cited` fallback path.

The fix: **only store cache files that a note actually references.** If the reflection
agent cited a URL but didn't create a note for it, the page wasn't important enough to
persist as a reference. Drop the `cited` flag from the "used" condition.

### The topic conventions gap

The reference-import skill tells the GHOST to "read a few files to pick a good topic
name" with no structure. No shared vocabulary, no check against existing topics, no
conventions beyond "topics are hierarchical."

---

## Design

### Fix 1: Only store note-referenced cache files

In `curate_references()`, change:

```rust
let used = file.cited || url_in_notes;
```

to:

```rust
let used = url_in_notes;
```

Cache files that are only `cited` (in agent findings) but not referenced by any note get
deleted instead of stored. The `cited` flag still matters for `classify_web_cache()` (it
controls preview extraction for the agent prompt), but it no longer triggers reference
storage.

This eliminates domain-slug orphan topics at the source. If a page is important, the
reflection agent should create a note that cites it — that note's tags determine the
topic.

### Fix 2: Shared topic vocabulary in skills

Add a "Topic Conventions" section to the reference-import skill (and reference it from
the note-writer skill) with concrete rules:

- **Check existing topics first**: run `ghost topics list` before creating a new topic.
  If a matching topic exists, use it.
- **Semantic names, not domain slugs**: `standing-desks` not `btod-com`. The topic
  describes the _subject_, not the _source_.
- **Depth**: `{category}` or `{category}/{collection}` — max 3 levels. Examples:
  `hardware/office`, `sports/sumo`, `llm/models`.
- **Alignment with notes**: reference topics should mirror note tag structure. If notes
  live under `hardware/office/`, references about the same subject go under
  `hardware/office/` too.
- **When the GHOST doesn't know**: for documents where the topic isn't obvious (PDFs,
  unfamiliar content), the GHOST should read the content first, then propose a topic to
  the operator if online. If autonomous (reflection), use the note's tag hierarchy.
