{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ghost.url = "github:mrtolkien/GHOST";
  };

  outputs = { nixpkgs, ghost, ... }:
    let
      mkEnv = system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in pkgs.buildEnv {
          name = "ghost-shell";
          paths = [
            ghost.packages.${system}.default
          ] ++ (with pkgs; [
            # Dev tools
            git gh curl wget jq ripgrep fd tree

            # Core POSIX utilities
            coreutils findutils bash gnugrep gnused gawk
            diffutils file less unzip gzip gnutar

            # Python + package manager
            uv python314

            # Database
            sqlite-interactive
          ]);
        };
    in {
      packages.x86_64-linux.default = mkEnv "x86_64-linux";
      packages.aarch64-linux.default = mkEnv "aarch64-linux";
    };
}
