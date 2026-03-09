{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { nixpkgs, ... }:
    let
      mkShell = system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in pkgs.mkShellNoCC {
          packages = with pkgs; [
            # Dev tools
            git
            gh
            curl
            wget
            jq
            ripgrep
            fd
            tree

            # Core POSIX utilities
            coreutils
            findutils
            bash
            gnugrep
            gnused
            gawk
            diffutils
            file
            less
            unzip
            gzip
            gnutar

            # Python + package manager
            uv
            python314

            # Database
            sqlite-interactive
          ];
        };
    in {
      devShells.x86_64-linux.default = mkShell "x86_64-linux";
      devShells.aarch64-linux.default = mkShell "aarch64-linux";
    };
}
