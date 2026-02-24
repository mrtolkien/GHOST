---
name: note-writer
description:
  Comprehensive guide for creating structured knowledge notes. Read this
  skill before writing any notes — it contains entity enumeration, note
  guidelines, title conventions, linking rules, archetypes, source quality
  notes, and decision notes.
---

# Note Writer — Knowledge Note Guide

This skill is the complete reference for writing structured knowledge notes. Follow
every section when creating or updating notes.

## Workflow

### 1. Enumerate Entities

Before writing any notes, list every distinct entity the source material explicitly
named, recommended, or compared. Each one gets its own note — don't merge related items
into a single note even if they're closely related or from the same category.

### 2. Create Notes

**Prioritize synthesized conclusions over raw data.** If Agent Findings are present, use
them as your primary source. If web cache files are present, read them with `read_file`
to extract concrete details.

**What to create** — scale to the richness of the input:

- **Entity notes** (archetype != topic): one per distinct person, project, concept,
  tool, or other concrete entity. Include specific details — names, numbers, versions,
  dates. Vague notes are useless.
- **Decision note**: if comparisons or trade-offs were discussed, link entity notes with
  rationale.
- **Source quality note**: if external sources were used, rate at least one source's
  reliability and depth. Tag under `{domain}/sources`. Title: "Source Name — Topic"
  since the same site may have different quality across domains.

Pass source URLs in the `sources` parameter of `note_write` — they will be preserved in
structured frontmatter. Do NOT put bare URLs in the note body.

Do NOT use `[[references/...]]` wiki links — references are managed automatically after
your session.

### 3. Verify Before Handoff

Before writing your handoff message, check your work against the entity list:

- Did you create or confirm a note exists for **every** entity you listed? If you missed
  any → go back to step 2.
- If external sources were used, did you create at least one **source quality note**? If
  not → step 2.
- If comparisons or trade-offs were discussed, did you create a **decision note**? If
  not → step 2.

## Note Guidelines

- **Atomic**: one concept per note, 100-400 words typical.
- **Specific**: exact names, numbers, versions, dates — never vague.
- **Linked**: `[[Title]]` for default edges, `[[rel>Title]]` for typed edges.
- **Tagged**: first tag = subfolder path (e.g. `rust/async`), lowercase,
  slash-separated. Keep tags to 2 levels max.
- **Trust**: start at 5, raise with evidence, lower for speculation (1-10).

## Titles

Follow Wikipedia naming conventions:

- **Short noun phrases**: "Tokio", not "The Tokio Async Runtime for Rust"
- **No prefixes**: "Async Runtime Comparison", not "Decision: Async Runtime"
- **No parenthetical qualifiers**: "Tokio", not "Tokio (Rust Runtime)"
- **Proper nouns as-is**: "Visual Studio Code", "Tom's Hardware"
- **Source notes — add topic**: "Source Name — Topic" when the source covers many
  domains (e.g. "Docs.rs — Async Ecosystem")

Consistent titles prevent duplicates and make wiki links predictable.

## Linking (critical)

Every entity note MUST contain at least one `[[wiki link]]`. If you mention another
entity by name, wrap it: `[[Entity Name]]`.

Common patterns:

- Entity notes → link related entities: `developed by [[Org Name]]`
- Comparison notes → link all compared items: `[[Option A]] vs [[Option B]]`
- Source quality notes → link domain context: `For [[Topic]] research...`

When creating entity notes under a topic, link UP to the topic note:
`Relevant to [[Topic Name]]`. This makes topic notes natural graph hubs with many
incoming edges.

Links create graph edges and stub notes. Use them liberally.

## Archetypes

| Archetype      | Purpose                                  |
| -------------- | ---------------------------------------- |
| `person`       | People, contacts, key individuals        |
| `concept`      | Ideas, definitions, mental models        |
| `decision`     | Choices with rationale and trade-offs    |
| `event`        | Meetings, occurrences, milestones        |
| `place`        | Locations, venues, geographic context    |
| `project`      | Projects, initiatives, ongoing work      |
| `organization` | Companies, teams, groups                 |
| `procedure`    | How-tos, workflows, step-by-step guides  |
| `media`        | Books, articles, films, podcasts         |
| `quote`        | Notable quotes with attribution          |
| `topic`        | Reference topic hubs for source material |

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4-6**: Reasonable confidence, based on experience or documentation
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by OPERATOR or primary sources

Start at 5 for most notes. Adjust as confidence changes.

## Tags

Use consistent, lowercase, hierarchical tags separated by slashes. Prefer existing tags
over creating new ones. Search what tags exist before creating notes.

## Rules

- Update existing notes over creating duplicates.
- Notes under ~1500 characters index as a single embedding vector — keep concise.
- Before creating notes, check existing folders and search for duplicates.
- Include `[[wiki links]]` for every entity mentioned — this builds the knowledge graph.
