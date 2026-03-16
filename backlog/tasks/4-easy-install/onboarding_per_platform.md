Should also add Signoz to the stack for observability.

---

In the standard deployment path, I want the ghost to be able to manage the extra
services: it should likely be a SKILL that:

- Explains the stack and how to manage it
- Has an extra with a list of important files and services running?

---

Review deployment:

- [ ] Installing and running on a Mac should be extremely easy, with GPU acceleration
      for llama.cpp + docling
  - Needs testing!
- [ ] Secondary target should be small Linux servers with no GPUs: in that case,
      embeddings and doclings should be configurable as remote services OR local
      services if perfs are acceptable (need to review docling RAM usage)
  - Needs implementation: could it work 100% in Docker?
- [ ] Third target should be small VPSs without a GPU and low RAM: in that case, there
      should be fallback for the services we run - Firecrawl for crawling, Brave API for
      web search (or still just run searxng, it's small), embeddings, docling, ...

---

Notes:

- Containers should use podman rootless
- Nix should setup garbage collection
- **`loginctl enable-linger <user>` is REQUIRED** on Linux for systemd user services.
  Without it, systemd kills all user services (including ghost-daemon) when the last
  login session ends. `ghost init` should run this automatically or at least warn.
- The daemon currently does not shut down gracefully within systemd's 90s timeout —
  SIGTERM is received but something blocks (likely Discord/serenity or SQLite workers).
  Need to add `TimeoutStopSec=` to the unit file and/or fix the shutdown path.
