---
name: note-writer
description:
  Create and manage structured knowledge notes, diary entries, and identity
  files. Use when you need to persist important information or maintain the
  knowledge base.
---

# Note Writer Skill

You are now in note-writing mode. This skill covers creating, updating, and managing
structured knowledge notes, diary conventions, and identity file management.

## When to Create Notes

Create notes when:

- The OPERATOR shares important context that should persist across sessions
- You discover something reusable (a pattern, a decision, a gotcha)
- Research results should be preserved for future reference
- A concept needs to be defined and tracked

Do NOT create notes for:

- Ephemeral chat context
- Reference material from external sources (use the reference-researcher skill)
- Diary entries (use diary conventions below)

## Archetypes

Archetypes are **optional** semantic classifications. Notes without an archetype are
valid unclassified notes. Set the `archetype` field in front matter (lowercase) when it
adds discoverability.

| Archetype      | Purpose                                 |
| -------------- | --------------------------------------- |
| `person`       | People, contacts, key individuals       |
| `concept`      | Ideas, definitions, mental models       |
| `decision`     | Choices with rationale and trade-offs   |
| `event`        | Meetings, occurrences, milestones       |
| `place`        | Locations, venues, geographic context   |
| `project`      | Projects, initiatives, ongoing work     |
| `organization` | Companies, teams, groups                |
| `procedure`    | How-tos, workflows, step-by-step guides |
| `media`        | Books, articles, films, podcasts        |
| `quote`        | Notable quotes with attribution         |

## Trust Scores

- **1-3**: Unverified, speculative, or from uncertain sources
- **4-6**: Reasonable confidence, based on experience or documentation
- **7-8**: Well-verified, cross-referenced with multiple sources
- **9-10**: Authoritative, confirmed by OPERATOR or primary sources

Start at 5 for most notes. Adjust as confidence changes.

## Tags

Use consistent, lowercase, hierarchical tags separated by slashes. Prefer existing tags
over creating new ones. Search what tags exist before creating notes.

- Good tags: `rust/library`, `architecture/decisions`, `debugging/patterns`
- Bad tags: `Important`, `TODO`, `misc`

## Note Length

Aim for atomic, information-dense notes:

- **Typical**: 100-400 words
- **Maximum**: ~1000 words
- Notes under ~1500 characters are indexed as a single embedding vector for precise
  retrieval. Keep notes concise to benefit from this optimization.

## Wiki Links

Link to related notes using `[[Title]]` or `[[Title|alias]]` syntax. Create links even
if the target note doesn't exist yet. Links enable graph-depth traversal during search.

## Updating vs. Creating

Before creating a new note, search first:

1. Search knowledge to check if a similar note exists
2. If found, update it to refine the content
3. If not found, create a new note

## Diary Conventions

Diary entries are date-based (`YYYY-MM-DD.md`), plain markdown with no front matter.
They are append-only.

- Use bullet points for events, decisions, and observations.
- Keep entries brief — details belong in notes, diary is the timeline.

## Identity Files

GHOST maintains three identity files in the workspace root:

- **BOOT.md**: Core personality, values, and behavioral constraints. Rarely changes.
  Only modify when explicitly directed by the OPERATOR.
- **SOUL.md**: Evolving self-model, communication style, and preferences. Update during
  reflection when significant self-awareness insights emerge.
- **OPERATOR.md**: Accumulated knowledge about the OPERATOR (preferences, context,
  communication style). Update when new OPERATOR information is captured.

These files are loaded into the system prompt context, so keep them concise and
well-structured. Edit them with `file_edit` or `write_file`.
