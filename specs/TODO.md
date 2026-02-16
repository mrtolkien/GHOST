# GHOST — PoC Roadmap

Ordered task list. Each task has a corresponding spec file. Complete them in order —
later tasks may depend on earlier ones.

## Phase 1: Foundation

- [ ] Project scaffolding and CLI skeleton — [01-scaffolding.md](01-scaffolding.md)
- [ ] Configuration system — [02-config.md](02-config.md)
- [ ] Observability setup (logfire + tracing) —
      [03-observability.md](03-observability.md)
- [ ] SurrealDB embedded setup — [04-database.md](04-database.md)

## Phase 2: Core Chat Loop

- [ ] Provider trait + OpenRouter adapter — [05-providers.md](05-providers.md)
- [ ] Kimi Code provider — [05a-kimi-code.md](05a-kimi-code.md)
- [ ] OpenAI OAuth provider (`ghost auth codex`) —
      [05b-openai-oauth.md](05b-openai-oauth.md)
- [ ] Chat orchestration and session management —
      [06-chat-orchestration.md](06-chat-orchestration.md)
- [ ] Context compaction — [07-compaction.md](07-compaction.md)
- [ ] System prompt rendering — [08-prompts.md](08-prompts.md)

## Phase 3: Interface

- [ ] Discord bot interface — [09-discord.md](09-discord.md)

## Phase 4: Tools and Skills

- [ ] Tool system (4 core + 3 reflection) — [10-tools.md](10-tools.md)
- [ ] Web module (library code) — [11-web-tools.md](11-web-tools.md)
- [ ] Skills system (agentskills.io) — [12-skills.md](12-skills.md)

## Phase 5: Knowledge

- [ ] Knowledge system with SurrealDB graph —
      [13-knowledge-system.md](13-knowledge-system.md)
- [ ] Embeddings integration (Ollama) — [14-embeddings.md](14-embeddings.md)
- [ ] Web cache and curation — [15-web-cache.md](15-web-cache.md)

## Phase 6: Jobs

- [ ] Job system: cron scheduling and execution — [16-jobs.md](16-jobs.md)
- [ ] Heartbeat and reflection subsystems — [17-default-jobs.md](17-default-jobs.md)

## Phase 7: Polish

- [ ] Integration test harness — [18-integration-tests.md](18-integration-tests.md)
- [ ] Coding agent — [19-coding-agent.md](19-coding-agent.md) _(NEEDS SPECS)_
