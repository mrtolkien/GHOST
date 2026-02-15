# Backlog — SurrealDB Client-Server Mode

## Overview

Support connecting to an external SurrealDB server instead of the embedded mode. This
enables:

- Shared database between multiple GHOST installations
- Database management via SurrealDB's tools (Surrealist UI, backups)
- Better performance for large knowledge bases

## Config

```toml
[database]
mode = "client"                      # "embedded" (default) or "client"
url = "ws://localhost:8000"          # SurrealDB WebSocket endpoint
namespace = "ghost"
database = "main"
username = "root"
password_env = "SURREALDB_PASSWORD"  # env var containing password
```

## Implementation

The `db` module should abstract over embedded vs client mode:

```rust
pub enum DbConnection {
    Embedded(Surreal<SurrealKv>),
    Client(Surreal<Client>),
}
```

All database queries should work identically regardless of mode — the SurrealDB API is
the same.

## Why Deferred

- Embedded mode is simpler and keeps the local-first philosophy
- Client mode requires running a separate SurrealDB process
- No immediate use case for multi-instance or remote access
