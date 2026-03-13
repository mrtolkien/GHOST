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

Should also add portainer to the stack maybe? Or Grafana? Or Signoz?

---

In the standard deployment path, I want the ghost to be able to manage the extra
services: it should likely be a SKILL that:

- Explains the stack and how to manage it
- Has an extra with a list of important files and services running?
