Review deployment:

- [x] Installing and running on a Mac should be extremely easy, with GPU acceleration
      for llama.cpp + docling
- [ ] Secondary target should be small Linux servers with GPUs -> like mine with a GTX
      1060
- [ ] Review docling install: currently broken on nixpkgs?

---

Notes:

- [x] Containers should use podman rootless
- [x] Nix should setup garbage collection
- [x] **`loginctl enable-linger <user>` is REQUIRED** on Linux for systemd user
      services. Without it, systemd kills all user services (including ghost-daemon)
      when the last login session ends. `ghost init` should run this automatically or at
      least warn.
