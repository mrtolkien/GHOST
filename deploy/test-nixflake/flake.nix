# Nix flake that wraps a pre-built ghost binary (simulating a GitHub release
# download) and bundles it with the shell tools GHOST needs at runtime.
#
# The ghost binary is patched with autoPatchelfHook so it runs on NixOS
# (which has no /usr/lib, /lib64, etc.).
#
# In production, `src = ./ghost-bin` would be replaced with a fetchurl from
# a GitHub release asset.
{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      mkPkgs = system:
        let
          pkgs = nixpkgs.legacyPackages.${system};

          # --- Ghost binary (pre-built, patched for NixOS) ---
          ghost-bin = pkgs.stdenv.mkDerivation {
            pname = "ghost-bin";
            version = "0.0.0";
            dontUnpack = true;

            nativeBuildInputs = [ pkgs.autoPatchelfHook ];

            buildInputs = [
              pkgs.glibc
              pkgs.gcc-unwrapped.lib  # libgcc_s.so.1
            ];

            installPhase = ''
              mkdir -p $out/bin
              cp ${./ghost-bin/ghost} $out/bin/ghost
              chmod +x $out/bin/ghost
            '';

            meta = {
              description = "Pre-built ghost binary patched for NixOS";
              platforms = [ "x86_64-linux" "aarch64-linux" ];
            };
          };

          # --- Combined environment: ghost + shell tools ---
          ghost-env = pkgs.buildEnv {
            name = "ghost-env";
            paths = [
              ghost-bin

              # Dev tools
              pkgs.git
              pkgs.gh
              pkgs.curl
              pkgs.wget
              pkgs.jq
              pkgs.ripgrep
              pkgs.fd
              pkgs.tree

              # Core POSIX utilities
              pkgs.coreutils
              pkgs.findutils
              pkgs.bash
              pkgs.gnugrep
              pkgs.gnused
              pkgs.gawk
              pkgs.diffutils
              pkgs.file
              pkgs.less
              pkgs.unzip
              pkgs.gzip
              pkgs.gnutar

              # Python + package manager
              pkgs.uv
              pkgs.python314

              # Database
              pkgs.sqlite-interactive
            ];
          };
        in
        {
          ghost = ghost-bin;
          default = ghost-env;
        };
    in
    {
      packages.x86_64-linux = mkPkgs "x86_64-linux";
      packages.aarch64-linux = mkPkgs "aarch64-linux";
    };
}
