---
title: External Dependencies
description:
  Why GHOST depends on external services and what each one does.
---

GHOST relies on a few external services. This page explains *why* each one exists and what
it provides.

:::note
In the future, all dependencies beyond the binary itself will be managed through Nix. For
now, you need to set them up manually — see [Installation](/getting-started/installation/)
for setup steps.
:::

## Ollama

**What**: Local LLM inference server
**Why**: GHOST uses Ollama to generate embeddings for semantic search. The
`qwen3-embedding:8b` model converts knowledge entries into vectors stored in sqlite-vec,
enabling similarity-based search alongside BM25 keyword matching.

**Used by**: Knowledge indexing, hybrid search

```bash
ollama pull qwen3-embedding:8b
```

## Crawl4AI

**What**: Headless browser for web page extraction
**Why**: Many web pages require JavaScript rendering or have complex layouts that simple
HTTP fetching can't handle. Crawl4AI runs a headless browser that extracts clean,
readable content from any page — even SPAs and dynamically-loaded content.

**Used by**: `web_fetch` tool, deep-research agent

```bash
docker run -d -p 11235:11235 unclecode/crawl4ai
```

## SearXNG

**What**: Self-hosted metasearch engine
**Why**: Privacy-respecting web search without API key dependencies. SearXNG aggregates
results from multiple search engines (Google, Bing, DuckDuckGo, etc.) without tracking.
It replaces the Brave Search API as the primary search backend.

**Used by**: `web_search` tool

## SQLite + sqlite-vec + FTS5

**What**: Embedded database with vector and full-text extensions
**Why**: GHOST stores everything in a single SQLite database — sessions, messages,
knowledge entries, embeddings. sqlite-vec provides KNN vector search for semantic
similarity. FTS5 provides full-text search with Porter stemming. No external database
server needed.

**Used by**: Everything — this is the core data layer
