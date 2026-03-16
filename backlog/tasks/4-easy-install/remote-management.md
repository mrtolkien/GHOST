Maybe we could have a dedicated onboarding CLI, different from the live CLI, that talks
to the daemon through a socket/port?

Those are a few different thoughts I've had that link to a common thread:

- How to interact with the config from outside the GHOST's container?

This likely translates into a lightweight CLI/TUI dedicated to it.

Could be linked to /home/tolki/Development/ghost/specs/backlog/tui.md

---

Currently, the `ghost` CLI operates only on the local ghost

But in practice, we'll use the CLI with a remote server

So:

- How to connect to the CLI to a remote server securily?
- Should we rewrite the CLI to talk to the daemon instead of directly touching the DB?
  - Would it be "simpler" to have a remote connection then?
- Should we have a distinct local and remote CLI?

---

This should of course go through Tailscale!
