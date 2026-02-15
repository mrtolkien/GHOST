# Backlog — Multi-Operator Support

## Overview

Allow multiple OPERATORS to interact with the same GHOST. This brings back the
approval/authentication flow from t-koma.

## Features

- OPERATOR registration and approval flow
- Per-OPERATOR rate limiting
- Per-OPERATOR session isolation
- OPERATOR.md per operator (or sections within it)
- Access levels (admin, user, read-only)

## Considerations

- Requires re-introducing an operator table in SurrealDB
- Identity files (OPERATOR.md) need to handle multiple operators
- Discord: multiple allowed user IDs
- Session mapping changes: session = (operator, channel)
- Knowledge: some notes might be operator-scoped

## Why Deferred

- Adds significant complexity to auth, permissions, and data model
- PoC focuses on single-user experience
- Multi-operator can be built on top without major refactoring (session chat already
  takes a session ID, just need to map operators to sessions)
