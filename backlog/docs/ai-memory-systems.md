# AI Memory Systems Review

Date: 2026-04-13

This note compares current AI memory systems with two goals:

1. Understand how each system is meant to be integrated into an agent.
2. Separate the parts that seem to materially help from the parts that look mostly like
   packaging or hype.

The strongest external anchor across this space is the LongMemEval benchmark itself, not
any single vendor write-up:

- LongMemEval benchmark paper: <https://openreview.net/forum?id=pZiyCaVuti>

## Common Failure Modes

Most memory systems are trying to address one or more of these real problems:

- Information loss from over-aggressive extraction or summarization.
- Stale facts that remain in memory with no validity model.
- Prompt cost and latency blowups from replaying too much history.
- Opaque retrieval, where the agent gets memory back but nobody can tell why.

When a system helps, it is usually because it improves one of those four things.

## Overall Take

The strongest mechanisms across the systems reviewed are:

- Keep some high-value traces verbatim instead of extracting everything.
- Keep a very small explicit always-visible state.
- Scope memory by user, project, agent, and session.
- Model time explicitly when facts can become stale.
- Make retrieval paths inspectable and debuggable.
- Bound prompt growth with compaction or stable prefixes.

The weakest claims are usually:

- "Self-evolving" or "self-improving" language without clear mechanism.
- Broad claims that "graph memory" or "agent memory" solves the problem by itself.
- Benchmark framing that hides the fact that the actual win came from a simpler
  baseline.

## MemPalace

### What it is

MemPalace is a local-first memory layer focused on storing conversation and project
history and retrieving it later through CLI, MCP, and hook-based integrations.

- Docs: <https://mempalace.github.io/mempalace/guide/getting-started.html>
- MCP integration: <https://mempalace.github.io/mempalace/guide/mcp-integration.html>
- Hooks: <https://mempalace.github.io/mempalace/guide/hooks.html>
- Repo: <https://github.com/MemPalace/mempalace>

### How it is integrated

Typical integration surfaces:

- CLI ingestion and search.
- MCP server for tool-based access.
- Auto-save hooks for Claude Code / Codex-like agents.

### How it stores and organizes data

MemPalace uses a spatial metaphor:

- wings
- rooms
- halls
- closets
- drawers

In practice, the important storage pieces are:

- verbatim text in ChromaDB for retrieval
- optional SQLite-backed knowledge graph / metadata
- local config and identity files

### How the agent should use it

The right usage pattern is:

1. On session boundaries or periodic hook events, ingest raw transcript chunks.
2. Before answering a question about prior work, run semantic search over verbatim
   memory.
3. Pull back the smallest number of supporting chunks that preserve original wording.
4. Treat the "palace" structure as optional filtering/navigation, not as the core recall
   engine.

This system is best when the agent needs:

- verbatim recall
- "why did we decide this?" answers
- local-first operation

It is not the right thing to trust for:

- aggressive compression
- relying on AAAK as the key win

### What seems genuinely useful

- Storing original text instead of forcing early extraction.
- MCP exposure and hook-based capture.
- Local-first operation with straightforward retrieval.

### What seems overstated

- The palace metaphor as a retrieval breakthrough.
- AAAK compression as the main value.

There is a notable independent reproduction discussion showing the strongest result came
from raw mode, while AAAK and room-based modes regressed:

- Reproduction issue: <https://github.com/MemPalace/mempalace/issues/39>
- Critical review: <https://vectorize.io/articles/mempalace-review>

### Bottom line

The real idea is good: keep important raw traces and search them. The branded structure
looks less important than the raw baseline.

## Mem0

### What it is

Mem0 is a selective memory pipeline: extract salient facts, deduplicate or update them,
store them, and retrieve them later. `Mem0g` adds a graph layer on top.

- Docs: <https://docs.mem0.ai/>
- Research: <https://mem0.ai/research>
- Repo: <https://github.com/mem0ai/mem0>

### How it is integrated

Typical agent pattern:

- `search` before generation
- inject returned memory into the prompt
- `add` after the turn

It also integrates via SDKs, frameworks, and MCP-style access.

### How it stores and organizes data

Core concepts:

- conversation memory
- session memory
- user memory
- organizational memory

Base Mem0 is primarily vector-memory plus extraction/update logic. Mem0g adds graph
entities and relations.

### How the agent should use it

The right runtime pattern is:

1. Before answering, query memory using the current user, agent, and run scope.
2. Inject only the returned facts, not the whole conversation history.
3. After the turn, run extraction on the new interaction.
4. Update, merge, or invalidate stored facts instead of blindly appending.

This system is best when the agent needs:

- personalization
- stable preference memory
- low latency
- low token overhead

Use graph mode only when the workload is actually relationship-heavy:

- multi-hop facts
- temporal relationships
- entity-centric recall

### What seems genuinely useful

- Extraction plus dedupe/update.
- Strict scoping by `user_id`, `agent_id`, and `run_id`.
- Layered memory types.

### What seems overstated

- The idea that graph mode changes everything.

The public Mem0 evidence suggests graph mode helps, but only incrementally relative to
the base selective-memory pipeline.

### Bottom line

The most valuable part is not the graph. It is the selective fact memory pipeline with
explicit scoping and updates.

## OpenMemory

### What it is

OpenMemory is a coding-agent-facing product layer on top of Mem0, aimed at Claude Code,
Claude Desktop, Cursor, VS Code, and other MCP-compatible clients.

- Product page: <https://mem0.ai/openmemory>
- OpenMemory docs: <https://docs.mem0.ai/openmemory/overview>
- OpenMemory site: <https://openmemory.ai/>

### How it is integrated

It is MCP-first and aimed at coding workflows:

- capture preferences and implementation patterns
- scope memory to project or repo
- inject relevant memory back into the coding assistant

### How it stores and organizes data

The visible model is:

- project-scoped or repo-scoped memories
- typed memories
- browse/edit/delete controls
- access logs

Under the hood, the retrieval substrate is still Mem0.

### How the agent should use it

The right runtime pattern is:

1. Treat each repo or project as its own memory scope.
2. Save only stable coding preferences, architectural decisions, recurring constraints,
   and local conventions.
3. Retrieve those memories at task start or when the query clearly relates to prior
   local decisions.
4. Keep a human-editable review surface so wrong memories can be fixed.

This system is best when the agent needs:

- coding preferences
- repo-specific conventions
- repeated task context across sessions

### What seems genuinely useful

- Project-scoped memory for coding assistants.
- Editable memory controls and logs.
- Better UX for Claude Code / MCP clients than raw Mem0 alone.

### What seems overstated

- The benchmark story is mostly inherited from Mem0, not OpenMemory specifically.
- Reliability claims for Claude/MCP should be treated cautiously until more independent
  validation exists.

### Bottom line

OpenMemory is mainly a useful productization layer for coding agents, not a separate
memory breakthrough.

## Zep / Graphiti

### What it is

Zep is the managed platform. Graphiti is the open-source temporal knowledge graph engine
underneath it.

- Zep vs Graphiti: <https://help.getzep.com/zep-vs-graphiti>
- Graphiti repo: <https://github.com/getzep/graphiti>
- Graphiti overview: <https://help.getzep.com/graphiti/graphiti/overview>

### How it is integrated

Two levels:

- Zep memory API for app teams that want a higher-level platform.
- Graphiti library or MCP server for teams that want graph-level control.

### How it stores and organizes data

Core model:

- episodes as provenance
- entities and relationships
- fact validity windows
- explicit invalidation instead of silent overwrite
- hybrid graph + lexical + semantic retrieval

### How the agent should use it

The right runtime pattern is:

1. Ingest new episodes from conversations, actions, and observations.
2. Extract facts tied to time and provenance.
3. Invalidate or end facts when new state supersedes old state.
4. Retrieve using a hybrid strategy when the question involves people, relations, or
   change over time.

This system is best when the agent needs:

- temporal correctness
- provenance
- relationship-heavy reasoning
- stale-fact management

### What seems genuinely useful

- Validity windows.
- Explicit fact invalidation.
- Provenance through episodes.
- Hybrid retrieval rather than graph-only retrieval.

### What seems overstated

- The broader claim that graph memory is a general replacement for simpler retrieval.

### Bottom line

If your memory problem is "facts change over time and wrong old facts are dangerous,"
this is one of the most concrete designs in the field.

## Letta

### What it is

Letta, formerly MemGPT, is a stateful agent framework built around explicit memory tiers
and agent-controlled memory editing.

- MemGPT paper: <https://arxiv.org/abs/2310.08560>
- Letta memory blocks:
  <https://docs.letta.com/guides/core-concepts/memory/memory-blocks>
- MemGPT architecture docs: <https://docs.letta.com/guides/agents/architectures/memgpt>
- Stateful agents: <https://docs.letta.com/guides/core-concepts/stateful-agents>

### How it is integrated

The agent is built with:

- always-visible memory blocks
- searchable archival memory
- conversation search
- compaction
- built-in tools to edit memory

This is closer to a full agent runtime than to a thin memory add-on.

### How it stores and organizes data

The important tiers are:

- memory blocks in active context
- recall memory for older conversation history
- archival memory for long-term searchable storage
- compacted summaries when context fills

### How the agent should use it

The right runtime pattern is:

1. Keep only a very small set of stable, editable memory blocks in-context at all times.
2. Push durable but non-critical facts into archival memory.
3. Search old conversations only when the current question points there.
4. Let compaction manage history growth, but treat memory blocks as the trusted summary
   layer.
5. Give the agent explicit rules for when it may edit memory blocks and when it must use
   archival memory instead.

This system is best when the agent needs:

- durable state
- explicit memory editing
- coordination across agents
- a clean separation between pinned state and searchable history

### What seems genuinely useful

- Editable always-visible memory blocks.
- Separate archival memory.
- Explicit compaction.
- Shared blocks across agents.

### What seems overstated

- The "LLM operating system" metaphor.

### Bottom line

The win is explicit state management, not the OS analogy.

## OpenViking

### What it is

OpenViking is a context database built around a filesystem-like abstraction for agent
context.

- Repo: <https://github.com/volcengine/OpenViking>
- Docs: <https://www.mintlify.com/volcengine/OpenViking/index>
- Storage: <https://www.mintlify.com/volcengine/OpenViking/concepts/storage>
- Viking URI: <https://www.mintlify.com/volcengine/OpenViking/concepts/viking-uri>

### How it is integrated

It supports MCP and multiple agent integrations. Claude Code is documented through an
example plugin in the repo:

- Claude Code plugin example:
  <https://github.com/volcengine/OpenViking/blob/main/examples/claude-code-memory-plugin/README.md>

The Claude Code example uses:

- `UserPromptSubmit` hook for auto-recall
- `Stop` hook for auto-capture
- MCP tools for explicit memory operations

### How it stores and organizes data

OpenViking organizes context under `viking://` URIs with scopes like:

- `resources`
- `user`
- `agent`
- `session`
- `queue`
- `temp`

It also has:

- AGFS for actual content and relations
- vector index for lookup
- `L0`, `L1`, `L2` summary layers
- first-class session memory extraction and commit

### How the agent should use it

The right runtime pattern is:

1. Store context into the correct URI scope instead of mixing everything together.
2. On prompt submit, search the most relevant scopes first, usually user and agent
   memory.
3. Pull back summaries first, then drill down only when needed.
4. On session completion, commit session data into durable user/agent memory.
5. Use the traversal path as a debugging artifact when recall quality is poor.

This system is best when the agent needs:

- inspectable retrieval
- hierarchical context
- explicit separation of user, agent, and session memory

### What seems genuinely useful

- Path-addressable context via `viking://`.
- Hierarchical `L0/L1/L2` summaries.
- Session-to-memory commit flow.
- Recursive retrieval with visible traversal.

### What seems overstated

- Broad "self-evolving" or universal context-database claims.

The benchmark evidence is promising but still narrow and mostly first-party.

### Bottom line

The most durable idea here is an inspectable filesystem-like control plane for context,
not the broader marketing language.

## Mastra Observational Memory

### What it is

Mastra Observational Memory is a memory mode inside Mastra that avoids per-turn
retrieval by maintaining a stable memory prefix plus recent live conversation.

- Research page: <https://mastra.ai/research/observational-memory>
- Blog post: <https://mastra.ai/blog/observational-memory>
- Implementation:
  <https://github.com/mastra-ai/mastra/tree/main/packages/memory/src/processors/observational-memory>

### How it is integrated

It runs two background memory workers:

- Observer
- Reflector

The main agent reads:

- stable memory prefix
- recent conversation tail

### How it stores and organizes data

It is text-first:

- append-only dated observation log
- reflection-based compaction
- no graph DB requirement
- no vector DB requirement

### How the agent should use it

The right runtime pattern is:

1. Keep a stable prefix that changes infrequently.
2. Append recent live events to the tail.
3. Let the observer convert recent tail content into structured observations.
4. Periodically compact older observations through reflection.
5. Avoid per-turn retrieval unless there is a clear need to break the model.

This system is best when the agent needs:

- prompt caching efficiency
- bounded context growth
- lower per-turn retrieval overhead

### What seems genuinely useful

- Stable prompt prefix.
- Thresholded compression.
- Dated event log.

### What seems overstated

- Anthropomorphic "subconscious" style framing.
- Benchmark claims that still have limited independent replication.

### Bottom line

This is one of the more practically interesting designs if prompt-cache stability is the
main bottleneck.

## Cross-System Comparison

### If the agent mainly needs verbatim recall

Prefer:

- MemPalace

### If the agent mainly needs compact personalized fact memory

Prefer:

- Mem0
- OpenMemory

### If the agent mainly needs stale-fact prevention and temporal reasoning

Prefer:

- Zep / Graphiti

### If the agent mainly needs explicit internal state management

Prefer:

- Letta

### If the agent mainly needs inspectable hierarchical retrieval

Prefer:

- OpenViking

### If the agent mainly needs prompt-cache stability and bounded context growth

Prefer:

- Mastra Observational Memory

## Recommended Design Lessons for Ghost

If Ghost adopts ideas from this space, the most defensible ones to copy are:

- Keep a small explicit always-visible state.
- Keep some verbatim history for high-value decisions and rationale.
- Scope memory by operator, session, project, and agent.
- Support explicit invalidation or end-dating of facts.
- Make recall paths inspectable.
- Avoid forcing every memory through extraction.
- Prefer incremental compaction over replaying the whole past.

The things to avoid copying without stronger evidence are:

- strong marketing metaphors as architecture
- graph-first designs without a clear temporal problem
- compression layers that degrade recall
- "self-improving memory" claims without a concrete update model
