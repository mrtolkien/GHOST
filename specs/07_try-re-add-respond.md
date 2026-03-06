Having structured response types would be nice:

- Sources / citations -> allows to show them much more cleanly (separate category,
  nicely formatted links, ...)
- Interactions from the GHOST: questions with pre-filled answers, yes/no triggers, ...
  - Needs to check how claude code does it's "questions" thing: is it a tool?

This is tangentially linked to security:

- The GHOST should be autonomous _but_ still ask for validation/feedback
- We might need to add a structured way to respond that allows for it
- For example, when asked to solve a problem, it should _ask_ before going deep into
  coding a solution for example. Or before creating a project. Or before doing an
  import. Or before spawning a deep research agent. Anything that costs a lot of tokens
  should likely go through a validation + clear speccing step

We removed the response tool in the past because of issues with e2e tests... But
reasoning was off, which was the real issue

Linked to /home/tolki/Development/ghost/specs/07b-message-source-linking.md
