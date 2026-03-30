# Optional Nix Dependencies System

## GOAL

Right now we're adding more and more services, scripts, and dependencies to the base
flake.

This is not good, we should properly make this GHOST-driven:

- We already have a python script, a shell env/system-management, and a service skill
  that explain how to install dependencies/write scripts
- We need to remove services and programs from the default services/flake to make them
  lighter
- We need to teach the GHOST to add those autonomously when needed, in the right skills:
  the compose files should be in skill/assets, the scripts in skill/scripts, ...
  - We have a script in ./services/docling, this is a mistake
- By default the GHOST should assume dependencies are there: it should follow a
  fail-fast approach before trying to add a dependency or service

## OLD NOTES

Rework the nix flake to support optional heavy dependencies (?docling?, pandoc, yt-dlp,
calibre, whisper.cpp) without installing them by default. User decides what they need.

Might be a secondary list in the flake (something like system_managed_packages = [...])
that we then concate-nate.

## Research Notes

- pandoc: ~283 MiB closure, very manageable
- yt-dlp: ~625 MiB closure (ffmpeg), fine
- calibre: ~2.8 GiB closure (Qt), heavy but only option for MOBI/AZW3
- whisper.cpp: ~1 GiB closure, clean single binary, best CPU-only option
- All are in nixpkgs
