GHOST relies on quite a few external services to work:

- Some native ones like llama.cpp/ollama, tailscale, docling
- Some direct docker containers like crawl4ai, a headless chrome instance, searxng
- A full-on Docker stack with Signoz that should be optional

Here's how I see it:

- During onboarding we should ask which services should be local and which should be
  remote (the user might want to run a single instance of the services for multiple
  GHOSTs for example)
- It should also be possible to skip services, particularly ones that require creating
  an account (tailscale)
- Native services should use Nix if possible to minimize dependencies, but for example
  tailscale has a great one-line installer... So imo for tailscale we should tell the
  user to run the tailscale installer/generate the installer from the web interface
- The GHOST should have a way to know which services it can manage: there should be a
  services skills AND they should be visible in the workspace: there should likely be a
  /deploy folder with docker compose files + a way to manage/list which other services
  are managed by the GHOST
- There should likely be compose files + services info bundled in the /assets, like we
  have default agents and all
