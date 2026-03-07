Currently, the `ghost` CLI operates only on the local ghost

But in practice, we'll use the CLI with a remote server

So:

- How to connect to the CLI to a remote server securily?
- Should we rewrite the CLI to talk to the daemon instead of directly touching the DB?
  - Would be "simpler" to have a remote connection then
- Should we have a distinct local and remote CLI?
