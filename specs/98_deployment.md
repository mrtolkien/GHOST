Review deployment:

- Installing and running on a Mac should be extremely easy, with GPU acceleration for
  llama.cpp + docling
- Secondary target should be small Linux servers with no GPUs: in that case, embeddings
  and doclings should be configurable as remote services OR local services if perfs are
  acceptable (need to review docling RAM usage)
