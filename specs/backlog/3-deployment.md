Review deployment:

- [x] Installing and running on a Mac should be extremely easy, with GPU acceleration
      for llama.cpp + docling
  - Needs testing!
- [ ] Secondary target should be small Linux servers with no GPUs: in that case,
      embeddings and doclings should be configurable as remote services OR local
      services if perfs are acceptable (need to review docling RAM usage)
  - Needs implementation: could it work 100% in Docker?

Should also add portainer to the stack maybe? Or Grafana?
