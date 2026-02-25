# Backlog — Reference Import Tool

## Overview

Bulk import documentation sites, code repositories, or web page collections into
searchable reference topics.

## User story

The user asks questions about dioxus. This is a cutting edge Rust UX library: the models
will always be extremely outdated.

We therefore want to give the model access to high quality data, but relying on web
search and fetch is slow and costly.

The right approach is therefore to:

- Find the documentation site, ideally the git repo
- Import it locally
- Compute embeddings for the documentation
- Use our knowledge search feature to search in the documentation

## Implementation

- The use of this tool should be hidden behind a skill. It's likely best to make it part
  of the `ghost` cli instead of a tool, since it will be used rarely.
- We need this reference data to be easy to query without polluting other results. It
  should likely be a combination of topic + collection:
  - `knowledge_search(topic="dioxus/docs", query="xyz", categories=["reference"])`
- There might be a need of structural changes to knowledge/references to make this
  feature clean. Do not hesistate to suggest improvements.

## Source Types

### Git

Clone a repository and import relevant files:

```json
{
  "source": "git",
  "url": "https://github.com/surrealdb/surrealdb",
  "paths": ["doc/", "README.md"],
  "extensions": [".md", ".rs"]
}
```

### Web (Single Page)

Import a single web page as a reference file.

### Crawl (BFS)

Crawl from a seed URL following same-host links.

This will be used for documentation sites that are not on git.

It should use the same logic as web_fetch (htmld, fallback to readablity, crawl4ai on
issues, ...)

```json
{
  "source": "crawl",
  "url": "https://docs.surrealdb.com",
  "max_depth": 3,
  "max_pages": 50
}
```

## Validation

The whole flow should be validated:

- We should validate that the command works well: it should import repos and create
  embeddings. The test should be behind a feature flag since it will be long and create
  a lot of embeddings.
- We should validate that the GHOST is able to discover this command, by reading the
  skill, then running the bash command with the right syntax
  - This should be a single test that stops at the bash command call
- We should validate that once the tool returns, the GHOST uses the knowledge search
  well. This should be a second test that starts after the return of the tool in the
  test above.
