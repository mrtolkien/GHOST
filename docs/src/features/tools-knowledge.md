# Knowledge Tools

Tools for interacting with GHOST's [knowledge base](knowledge.md).

## knowledge_search

Hybrid search combining BM25 full-text and semantic embeddings.

| Parameter    | Type    | Required | Description                                                                   |
| ------------ | ------- | -------- | ----------------------------------------------------------------------------- |
| `query`      | string  | yes      | Search query                                                                  |
| `categories` | array   | no       | Filter by type: `notes`, `references`, `diary`. Default: `["notes", "diary"]` |
| `limit`      | integer | no       | Max results. Default: 10                                                      |

## note_write

Create or update structured knowledge notes. Available only in reflection contexts.

| Parameter   | Type    | Required | Description                               |
| ----------- | ------- | -------- | ----------------------------------------- |
| `action`    | string  | yes      | `create` or `update`                      |
| `title`     | string  | yes      | Note title                                |
| `body`      | string  | yes      | Markdown body (supports `[[wiki links]]`) |
| `archetype` | string  | no       | Note type (see below)                     |
| `tags`      | array   | no       | Tags for categorization                   |
| `sources`   | array   | no       | Source URLs for attribution               |
| `trust`     | integer | no       | Confidence level 1-10. Default: 5         |

**Archetypes**: person, concept, decision, event, place, project, organization,
procedure, media, quote, topic
