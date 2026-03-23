- ghost services
  - Lists services (saved as toml in /services) and how they are run (nix, docker,
    podman, ...). The goal is to have a dedicated way to list services _managed_ by
    GHOST.
  - The services.toml needs to be generated during init and should be editable through
    the CLI (ghost services add)
  - Mostly a way to validate it's all working, not a real way to manage (left up to
    shell tool + the proper commands of nix/docker/podman)
- ghost start/stop
  - Simple way to stop the daemon + associated services
  - should ghost daemon also start the services? I'd say no
