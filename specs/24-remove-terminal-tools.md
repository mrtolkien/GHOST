# Spec 24: Remove respond and report_findings Terminal Tools

## Problem

The `respond` and `report_findings` tools are "terminal tools" — intercepted by the tool
loop before execution. They were designed to force structured output (message +
citations) through the tool-calling mechanism, but they cause more problems than they
solve:

1. **Prompt pollution**: every model sees these tools in its schema, adding complexity.
   Models sometimes call `respond` when they should just write text, or get confused
   about when to use `report_findings` vs plain text output.
2. **Fragile interception**: the tool loop has special-case code to detect these tools,
   parse their arguments, filter them from stored tool_calls, and handle rejection
   (report_findings minimum fetch check). This adds ~60 lines of branching logic.
3. **Agent prompt coupling**: the deep research agent prompt has 9 references to
   `report_findings`. When the tool was available, the model used it. When removed from
   `all_available()` but still in the prompt, the model tried and failed. The prompt
   should just say "write your report as plain text."
4. **Citations work without a tool**: citations can be extracted from tool_use history
   (web_fetch URLs, knowledge_search results) rather than requiring the model to
   enumerate them in a tool call.

## Changes

### Remove tool files

- Delete `src/tools/respond.rs`
- Delete `src/tools/report_findings.rs`
- Remove `pub mod respond` and `pub mod report_findings` from `src/tools/mod.rs`
- Remove `pub use` of `RESPOND_TOOL_NAME` and `REPORT_FINDINGS_TOOL_NAME`

### Remove from tool manager

- `src/tools/manager.rs`: remove `respond` and `report_findings` registrations from
  `for_chat()` and `all_available()`. Tool count drops.

### Simplify tool loop

- `src/chat/tool_loop.rs`: remove `respond` and `report_findings` interception blocks.
  Remove `count_web_fetches()`. Remove `MIN_REPORT_FETCHES` constant. Remove imports.
  The `ToolLoopHandler` trait loses `on_respond()` — `on_end_turn()` handles all
  terminal responses.

### Simplify handler implementations

- `src/chat/session.rs`: remove `on_respond()` from `ChatHandler`, `AgentHandler`, and
  `JobHandler`. Remove the terminal-tool filtering in `AgentHandler::on_end_turn()`.
  Remove `Citation` handling from `on_respond` (citations become a post-processing
  concern, not a tool-loop concern).

### Remove Citation from tool loop types

- `src/chat/types.rs`: if `Citation` is only used by the respond tool path, remove it or
  move it to where it's actually needed (web cache resolution).

### Update tests

- `tests/chat_orchestration.rs`: remove tests for `respond` tool interception
  (`structured_output_populates_citations_and_creates_edges`,
  `web_cache_citation_resolves_url_from_frontmatter`). These test the respond tool
  mechanism, not chat orchestration.
- `tests/tools.rs`: update tool count assertion.

### Update agent prompt

- `prompts/agents/deep-research.md`: remove `report_findings` from tools list in
  frontmatter. Remove all references to `report_findings` in the prompt body. Replace
  with "write your complete report as plain text in your final message."
- Remove `read_file` from the tools list (not used by the agent in practice).

### Update chat system prompt

- `prompts/chat-system.md`: remove any references to the `respond` tool. The GHOST
  should respond with plain text, not through a tool call.

## Non-Goals

- Replacing the citation mechanism — citations from web_fetch URLs can still be tracked
  via the web cache system. Just not via a tool call.
- Changing the minimum fetch enforcement for agents — this moves to the prompt (the
  agent's self-check section already has it) rather than being enforced in code.

## Files

| File                              | Change                            |
| --------------------------------- | --------------------------------- |
| `src/tools/respond.rs`            | DELETED                           |
| `src/tools/report_findings.rs`    | DELETED                           |
| `src/tools/mod.rs`                | Remove modules + re-exports       |
| `src/tools/manager.rs`            | Remove tool registrations         |
| `src/chat/tool_loop.rs`           | Remove interception + simplify    |
| `src/chat/session.rs`             | Remove on_respond, simplify       |
| `src/chat/types.rs`               | Remove/move Citation if unused    |
| `src/chat/convert.rs`             | Remove parse_respond_call         |
| `prompts/agents/deep-research.md` | Remove report_findings references |
| `prompts/chat-system.md`          | Remove respond tool references    |
| `tests/chat_orchestration.rs`     | Remove respond tool tests         |
| `tests/tools.rs`                  | Update tool count                 |
