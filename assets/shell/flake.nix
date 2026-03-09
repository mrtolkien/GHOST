{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager = {
      url = "github:nix-community/home-manager";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, home-manager, ... }:
    let
      mkGhostHome = system:
        home-manager.lib.homeManagerConfiguration {
          pkgs = nixpkgs.legacyPackages.${system};
          modules = [{
            home.username = "root";
            home.homeDirectory = "/root";
            home.stateVersion = "24.11";

            home.packages = with nixpkgs.legacyPackages.${system}; [
              # Dev tools
              git gh curl wget jq ripgrep fd tree

              # Core POSIX utilities
              coreutils findutils bash gnugrep gnused gawk
              diffutils file less unzip gzip gnutar

              # Python + package manager
              uv python314

              # Database
              sqlite-interactive
            ];

            home.sessionVariables = {
              # Add custom env vars here
            };

            programs.home-manager.enable = true;
          }];
        };
    in {
      homeConfigurations."ghost-x86_64" = mkGhostHome "x86_64-linux";
      homeConfigurations."ghost-aarch64" = mkGhostHome "aarch64-linux";
    };
}
