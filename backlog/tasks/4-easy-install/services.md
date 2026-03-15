Create a clean services list and docker compose file to be included in the binary and
deploy it with podman rootless:

- crawl4ai
- searxng (also possible native with nix, but no gain?)
- Headless chrome w/ CDP

Native would be better for:

- Docling (maybe even use the CLI? Can we install it with nix as part of the flake?)
- Llama.cpp
