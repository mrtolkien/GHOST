# Optional Nix Dependencies System

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
