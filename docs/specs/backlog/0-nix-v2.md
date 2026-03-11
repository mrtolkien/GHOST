We need the ghost binary to be part of the nix flake:

- The docker image becomes a "shell" that bootstraps the flake
- Then the actual default flake kicks in and installs the latest GHOST version
- Binaries should be available through github, and pinned with tags/versions
- Updating should just be nix flake update
