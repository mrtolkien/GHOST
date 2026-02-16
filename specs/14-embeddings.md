# 14 — Embeddings Integration (Ollama)

## Overview

Embeddings power the semantic search side of the knowledge system. The GHOST uses a
local Ollama server to generate embedding vectors, which are stored in SurrealDB for
vector similarity search.

## Architecture

```
Note/Reference/Diary content
    ↓ (chunk if needed)
Ollama embedding API
    ↓
Embedding vectors (float[])
    ↓
SurrealDB vector index
```

## Ollama Integration

### API

```
POST http://127.0.0.1:11434/api/embed
Content-Type: application/json

{
  "model": "qwen3-embedding:8b",
  "input": ["text to embed", "another text"]
}
```

Response:

```json
{
  "model": "qwen3-embedding:8b",
  "embeddings": [[0.1, 0.2, ...], [0.3, 0.4, ...]]
}
```

### Batching

Send multiple texts in a single request (up to `embeddings.batch_size`, default 32).
This significantly improves throughput during bulk indexing.

## Chunking

Short notes (under ~1500 characters) are embedded as a single vector for precise
retrieval. Longer content is chunked:

- Chunk size: ~1000 characters
- Overlap: ~200 characters
- Split at paragraph boundaries when possible, fall back to sentence boundaries

Tags are prepended to the first chunk to boost tag-relevant search results.

## SurrealDB Vector Storage

```surql
-- Embedding vectors table
DEFINE TABLE embedding SCHEMAFULL;
DEFINE FIELD source_table ON embedding TYPE string;  -- "note", "reference", "diary"
DEFINE FIELD source_id ON embedding TYPE record;
DEFINE FIELD chunk_index ON embedding TYPE int;
DEFINE FIELD chunk_text ON embedding TYPE string;
DEFINE FIELD vector ON embedding TYPE array<float>;
DEFINE FIELD created_at ON embedding TYPE datetime;

-- Vector similarity index
DEFINE INDEX idx_embedding_vector ON embedding FIELDS vector
    MTREE DIMENSION 1024   -- adjust to model's dimension
    DIST COSINE;
```

### Vector Search Query

```surql
SELECT *, vector::similarity::cosine(vector, $query_vector) AS score
FROM embedding
WHERE vector <|10|> $query_vector
ORDER BY score DESC;
```

## Indexing Pipeline

### On Note Create/Update

1. Chunk the note content
2. Prepend tags to the first chunk
3. Generate embeddings via Ollama
4. Delete old embeddings for this note
5. Insert new embeddings

### On Reference Create

1. Chunk the reference content
2. Generate embeddings via Ollama
3. Insert embeddings

### On Diary Write

1. Embed the full diary entry (typically short enough for one vector)
2. Upsert embedding for today's entry

### Bulk Re-index

Provide a `ghost knowledge reindex` CLI command for rebuilding all embeddings (e.g.,
after changing the embedding model).

## Hybrid Search Scoring

Combine full-text (BM25) and embedding (cosine similarity) scores:

```rust
pub struct SearchResult {
    pub id: String,
    pub title: Option<String>,
    pub snippet: String,
    pub score: f64,           // Combined score
    pub bm25_score: f64,      // Full-text score
    pub embedding_score: f64, // Vector similarity score
    pub source: SearchSource, // Note, Reference, or Diary
}

fn combine_scores(bm25: f64, embedding: f64) -> f64 {
    // Weighted combination — tune these weights based on experience
    0.4 * bm25 + 0.6 * embedding
}
```

## Config

```toml
[embeddings]
url = "http://127.0.0.1:11434"
model = "qwen3-embedding:8b"
batch_size = 32
```

## Graceful Degradation

If Ollama is not available:

- Log a warning on startup
- Knowledge search falls back to BM25 only (no embeddings)
- Note creation/update skips embedding generation
- Provide a CLI command to backfill embeddings when Ollama becomes available

## Validation

1. `cargo test --features live-tests` — generate an embedding for a short text via
   Ollama, verify the vector has the expected dimension (requires Ollama running)
2. `cargo test --features live-tests` — create two semantically similar notes and one
   unrelated note, run vector search, verify the similar notes rank higher
3. `cargo test --features live-tests` — hybrid search: verify combined BM25 + embedding
   scores produce better ranking than either alone
4. `cargo test` — graceful degradation: with Ollama unavailable, knowledge search falls
   back to BM25-only without errors
5. `cargo test --features live-tests` — `ghost knowledge reindex` rebuilds all
   embeddings (create notes, reindex, verify vectors exist)
6. `just ci` — passes

## Acceptance Criteria

- Embeddings are generated via Ollama HTTP API
- Notes, references, and diary entries are embedded on write
- Vector search returns semantically similar results
- Hybrid search combines BM25 and embedding scores
- Batch embedding works for bulk operations
- `ghost knowledge reindex` rebuilds all embeddings
- System works (degraded) when Ollama is unavailable
- All embedding operations produce tracing spans with model, batch size, and duration
- `just ci` passes

## Prior Art

Old code in `../t-koma`:

- `t-koma-knowledge/src/engine/` — Embedding generation via Ollama HTTP API, chunking
  strategy (paragraph-aware splitting with overlap), batch processing. Directly reusable
  logic, just change the vector storage from sqlite-vec to SurrealDB's MTREE index.
