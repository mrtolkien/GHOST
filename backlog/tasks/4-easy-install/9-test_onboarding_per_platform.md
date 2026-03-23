Review deployment:

- [ ] Installing and running on a Mac should be extremely easy, with GPU acceleration
      for llama.cpp + docling
  - Needs to be tested!
- [ ] Secondary target should be small Linux servers with GPUs -> like mine with a GTX
      1060
- [ ] Third target should be small VPSs without a GPU and low RAM: in that case, there
      should be fallback for the services we run - Firecrawl for crawling, Brave API for
      web search (or still just run searxng, it's small), embeddings, docling, ...

---

Notes:

- [x] Containers should use podman rootless
- [x] Nix should setup garbage collection
- **`loginctl enable-linger <user>` is REQUIRED** on Linux for systemd user services.
  Without it, systemd kills all user services (including ghost-daemon) when the last
  login session ends. `ghost init` should run this automatically or at least warn.
