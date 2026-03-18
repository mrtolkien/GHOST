Review and expand the language/extension allowlist for code embedding.

## Current Allowlist

Extensions embedded by the file watcher when walking `code/<slug>/` repos:

**Tree-sitter supported (AST-aware chunking):** `.rs`, `.py`, `.js`, `.ts`, `.tsx`,
`.jsx`, `.go`, `.sh`, `.bash`, `.toml`, `.json`

**Common languages without tree-sitter grammars yet:** `.c`, `.h`, `.cpp`, `.hpp`,
`.java`, `.kt`, `.rb`, `.sql`, `.lua`, `.zig`, `.ex`, `.exs`, `.yaml`, `.yml`, `.md`

These fall back to text-based chunking (line splitting) which still works for embeddings
but loses structural awareness.

## Candidates to Add

Languages worth considering for tree-sitter grammar support in the chunker:

- **C/C++** — tree-sitter-c, tree-sitter-cpp (widely used, mature grammars)
- **Java/Kotlin** — tree-sitter-java, tree-sitter-kotlin
- **Ruby** — tree-sitter-ruby
- **Zig** — tree-sitter-zig
- **Elixir** — tree-sitter-elixir
- **Lua** — tree-sitter-lua (relevant: Ghost uses Lua for agents)
- **SQL** — tree-sitter-sql (useful for migration files)
- **Haskell** — tree-sitter-haskell
- **Scala** — tree-sitter-scala
- **Swift** — tree-sitter-swift
- **PHP** — tree-sitter-php
- **CSS/SCSS** — tree-sitter-css
- **HTML** — tree-sitter-html
- **Dart** — tree-sitter-dart

## Considerations

- Each tree-sitter grammar adds compile-time cost and binary size
- Text-based fallback still produces useful embeddings — AST chunking is a quality
  improvement, not a hard requirement
- Prioritize languages the OPERATOR actually uses (configurable allowlist?)
- Consider making the extension list configurable in config.toml rather than hardcoded
