Those are a few different thoughts I've had that link to a common thread:

- How to interact with the config from outside the GHOST's container?

Since containerized execution is the golden path, it's important to be able to configure
the GHOST from _outside_ its environment.

This likely translates into a lightweight CLI/TUI dedicated to it.

Could be linked to /home/tolki/Development/ghost/specs/backlog/tui.md

---

We need a good onboarding flow:

- ?Check nix install?
- Setup model + embeddings (OpenRouter does both!)
- Setup discord (bot token + approved user id)
- Setup logfire/opentelemetry -> Optional
- Setup tailscale -> Optional atm, will likely be required in the future for web
  interface

---

Maybe we could have a dedicated onboarding CLI, different from the live CLI, that talks
to the daemon through a socket/port?

---

- Onboarding should include oauth sync
- Onboarding/cli config picker should properly list available models for all providers
  - For example, get top models on openrouter, ...
- Onboarding/deployment should work on Linux with all GPU types (Nvidia, AMD, Intel,
  ...)

---

Currently, the `ghost` CLI operates only on the local ghost

But in practice, we'll use the CLI with a remote server

So:

- How to connect to the CLI to a remote server securily?
- Should we rewrite the CLI to talk to the daemon instead of directly touching the DB?
  - Would it be "simpler" to have a remote connection then?
- Should we have a distinct local and remote CLI?
