---
name: knowledge-navigator
description:
  Navigate and query the knowledge base effectively. Use when you need to
  search existing knowledge, browse references by topic, explore the
  knowledge graph, or check for orphan notes.
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

## Workflow Tips

1. **Before creating notes**, always search first to avoid duplicates.
2. **Use graph traversal** to understand how a topic connects to existing knowledge.
3. **Check orphans** periodically — orphan notes should be linked into the graph.
4. **Browse references** by topic to find source material for a domain.
5. **Use tags** to discover knowledge clusters and find related content.
