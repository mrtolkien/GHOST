# Continue/Stop Buttons on Tool Iteration Limit

When the tool loop hits `MaxIterations`, show Discord buttons (Continue / Stop) instead
of a plain text message, so the user can click to continue without typing.

## Design

### 1. `SessionChat::continue_session()` (new method)

Clone of `chat_with_images` minus "create user message in DB" step: acquire session
guard -> load history -> create ChatHandler -> run_tool_loop() -> release guard. Returns
`(ChatResult, RunMetadata)`.

### 2. Button rendering (bot.rs)

Replace the `send_gateway_v2()` call at the `MaxIterations` check with
`send_v2_message()` containing:

- `text_display("Reached tool iteration limit.")`
- `action_row([button("Continue", "continue_{session_id}", 1), button("Stop", "stop_{session_id}", 2)])`
- Wrapped in `container()` with `WARNING_EMBED_COLOR`

### 3. Interaction routing (bot.rs `interaction_create`)

New `custom_id` prefix handling:

- `"continue_{session_id}"` -> spawn async task calling
  `session_chat.continue_session()`, send result same way `handle_message` does
  (response + check for another MaxIterations)
- `"stop_{session_id}"` -> acknowledge only (buttons disappear). No further action.

### 4. Button cleanup

Discord automatically disables components on acknowledged interactions. The existing
`Acknowledge` response handles this.

### Error handling

If `continue_session` fails (session busy race), send a gateway warning in the channel.

### What doesn't change

- Tool loop internals, ChatStopReason, ToolLoopHandler, history loading
- Continuation is semantically identical to user sending a message, minus DB user
  message
- Other interfaces (CLI) unaffected
