# Backlog — Built-in Embeddings via Candle

## Overview

Replace Ollama dependency for embeddings with a built-in model using the `candle`
framework. This eliminates the need for an external Ollama server.

## Motivation

- One less external dependency for end users
- Faster startup (no need to wait for Ollama)
- More predictable performance (no HTTP overhead)
- Smaller deployment footprint

## Proposed Design

### Model

Use a small, efficient embedding model that can run on CPU:

- `all-MiniLM-L6-v2` (384 dimensions, ~80MB) — fast, good quality
- `bge-small-en-v1.5` (384 dimensions, ~130MB) — better quality
- Or another model based on benchmarks at the time of implementation

### Integration

```rust
pub enum EmbeddingBackend {
    Ollama(OllamaEmbedder),
    Candle(CandleEmbedder),
}

pub struct CandleEmbedder {
    model: BertModel,
    tokenizer: Tokenizer,
}

impl CandleEmbedder {
    pub fn new(model_path: &Path) -> Result<Self> { ... }
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> { ... }
}
```

### Model Download

On first use, download the model from Hugging Face Hub to a cache directory:
`~/.cache/ghost/models/`

### Config

```toml
[embeddings]
backend = "candle"          # or "ollama"
model = "all-MiniLM-L6-v2"  # for candle
# url = "..."               # for ollama
```

## Dependencies

- `candle-core`, `candle-nn`, `candle-transformers`
- `tokenizers` (Hugging Face tokenizer)
- `hf-hub` (model download)

## Considerations

- Binary size increase (~10-20MB for candle)
- CPU-only for portability (GPU support as optional feature)
- First embedding call will be slow (model loading)
- Need to benchmark against Ollama for quality and speed
