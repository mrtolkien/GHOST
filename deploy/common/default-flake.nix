{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            git
            gh
            curl
            wget
            jq
            ripgrep
            fd
            tree
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
          ];
        };
      });
}
