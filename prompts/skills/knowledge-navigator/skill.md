---
name: knowledge-navigator
description:
  Knowledge system internals — graph traversal, orphan detection, topic browsing,
  reference listing, and advanced query techniques. Not needed for basic searches or
  note writing.
---

# Knowledge Navigator Skill

This skill teaches you how to use the knowledge system's query capabilities.

## Searching

Use `knowledge_search` for hybrid BM25 + semantic search. It defaults to notes and diary
— pass `categories: ["notes", "references", "diary"]` to include references.

For CLI access: `ghost knowledge search "query" [--kind note|reference|diary]`

## Browsing References

References are organized by topic (subdirectory name under `references/`).

CLI commands:

- `ghost knowledge references` — list all references, grouped by topic
- `ghost knowledge references --topic rust` — list references for a specific topic
- `ghost knowledge references --limit 50` — increase result limit

## Graph Traversal

The knowledge graph connects notes via typed edges (`[[wiki links]]`) and citations.

CLI commands:

- `ghost knowledge graph "Note Title"` — show incoming and outgoing edges
- `ghost knowledge graph "Note Title" --direction out` — outgoing edges only
- `ghost knowledge graph "Note Title" --direction in` — incoming edges only
- `ghost knowledge graph --orphans` — find notes with no connections
- `ghost knowledge graph --stats` — edge and stub counts

## Tags

- `ghost knowledge tags` — list all tags with counts

## Recent Activity

- `ghost knowledge recent [--limit 20]` — recently updated knowledge items

## Stats

- `ghost knowledge stats` — counts of notes, references, diary entries, edges, tags, and
  embeddings

## Reference Topics

Topics are pure namespaces that group both notes and references. Notes can have an
explicit `topic_id` linking them to a topic namespace. Import metadata (source URL,
version) is stored in separate `import_batch` records, not on the topic itself.

References are organized by topic. Use the `topic` parameter for scoped search:

- `knowledge_search(query="hooks", topic="dioxus")` — search all dioxus sub-topics
- `knowledge_search(query="pool", topic="sqlx/api")` — search a specific sub-topic
- `knowledge_search(query="framework", categories=["topics"])` — search topics by name

CLI:

- `ghost knowledge search "query" --topic dioxus` — topic-scoped search
- `ghost topics list` — list all topics with note and reference counts
- `ghost topics search "query"` — search topics by name
- `ghost reference import git --url <url> --topic <name>` — import git references
- `ghost document import url --url <url> --topic <name>` — import documents

## Direct SQL Access

The `schema.sql` extra included with this skill contains the full table definitions for
the SQLite database. If the CLI commands above are not sufficient for debugging or
advanced queries, you can use `sqlite3` directly against the workspace database:

```bash
sqlite3 ~/GHOST/ghost.db "SELECT count(*) FROM note"
```

Consult the schema extra to understand table structure, column names, and relationships
before writing queries.

## Workflow Tips

1. **Before creating notes**, always search first to avoid duplicates.
2. **Use graph traversal** to understand how a topic connects to existing knowledge.
3. **Check orphans** periodically — orphan notes should be linked into the graph.
4. **Browse references** by topic to find source material for a domain.
5. **Use tags** to discover knowledge clusters and find related content.
