# Concepts Overview

Quick reference for GHOST's core concepts. Each links to its detailed chapter.

| Concept                                | Summary                                                                                          |
| -------------------------------------- | ------------------------------------------------------------------------------------------------ |
| **OPERATOR**                           | The human user. One per installation. Identified by Discord user ID.                             |
| **GHOST**                              | The AI agent. Identity defined by workspace files. One per installation.                         |
| **[Identity](ghost/identity.md)**      | BOOT.md, SOUL.md, OPERATOR.md — files that define your GHOST's personality and your preferences. |
| **[Provider](ghost/providers.md)**     | LLM backend (OpenRouter, Kimi Code, OpenAI OAuth). Configurable model aliases.                   |
| **[Interface](ghost/interfaces.md)**   | Communication transport. Discord is the primary interface.                                       |
| **[Session](features/chat.md)**        | A chat thread between OPERATOR and GHOST. Maps 1:1 to a Discord thread.                          |
| **[Knowledge](features/knowledge.md)** | Notes, references, and diary entries stored in SurrealDB with graph edges and embeddings.        |
| **[Skill](features/skills.md)**        | Workflow file (agentskills.io format) that teaches your GHOST how to handle specific tasks.      |
| **[Agent](features/agents.md)**        | Autonomous background worker for complex multi-step tasks (research, reflection).                |
| **[Job](features/jobs.md)**            | Cron-scheduled markdown file that runs on a timer.                                               |
| **[Tool](features/tools-core.md)**     | A capability the GHOST can invoke during chat (file I/O, search, web fetch, etc.).               |
