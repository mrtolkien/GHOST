# Prompt Design Philosophy

## Layered Architecture

The GHOST prompt system is layered. Each layer has a clear purpose:

1. **System prompt** (`prompts/chat-system.md`): Generic behavioral rules, tool
   documentation, core principles. Never contains specialist knowledge or
   domain-specific workflows. Conciseness is a core goal — every line competes for the
   model's attention budget.

2. **Skills** (`skills/*.md`): Specific workflows triggered by matching the OPERATOR's
   request to a skill description. Skills contain the detailed instructions the model
   needs for that workflow (when to spawn agents, which sources to check, what steps to
   follow). Skills are the primary mechanism for teaching the GHOST specialized
   behavior.

3. **Agents** (`agents/*.md`): Dedicated prompts for background agent sessions. Used for
   complex, multi-step workflows that benefit from isolated context (deep research,
   reflection). Agent prompts can include progress rules in TOML frontmatter for runtime
   enforcement.

4. **Identity files** (`BOOT.md`, `SOUL.md`, `OPERATOR.md`): Personality, preferences,
   and operator context. Injected into the system prompt via template variables.

## Key Principles

- **System prompt stays generic**: Adding specialist knowledge to the system prompt is a
  design smell. It bloats context, creates interference between unrelated domains, and
  makes prompt maintenance harder. If a behavior is domain-specific, it belongs in a
  skill.

- **Conciseness reduces context fatigue**: Long system prompts cause models to lose
  track of individual rules. Every section must earn its place. Remove advisory text,
  merge redundant instructions, front-load the most important rules.

- **Skills must have strong descriptions**: The model decides whether to read a skill
  based entirely on its frontmatter `description` field. A weak description means the
  model skips the skill and answers from training data. Descriptions should name the
  specific scenarios that trigger the skill and warn about the consequences of not
  reading it.

- **The system prompt must emphasize skill-reading**: The system prompt should make it
  clear that skills exist and must be consulted before responding to matching requests.
  This is the bridge between the generic system prompt and the specific skill
  instructions.

- **Complex workflows get dedicated agents**: When a workflow requires reading many
  pages, maintaining a long chain of reasoning, or running for minutes, it belongs in a
  background agent with its own prompt — not in the main chat context where it would
  bloat the conversation history.

## Debugging Flaky Behavior

When the model inconsistently follows a rule:

1. Check if the rule is in the right layer (system prompt vs skill vs agent)
2. Check if the skill description is strong enough to trigger reading
3. Check if the system prompt emphasizes skill-reading adequately
4. Check for context fatigue — is the prompt too long? Are important rules buried?
5. Consider runtime enforcement (progress rules, post_tool_iteration nudges) as a last
   resort for critical behaviors the model tends to skip
