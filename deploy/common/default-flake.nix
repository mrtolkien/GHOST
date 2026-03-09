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
      system = builtins.currentSystem;
      pkgs = nixpkgs.legacyPackages.${system};
    in {
      homeConfigurations.ghost = home-manager.lib.homeManagerConfiguration {
        inherit pkgs;
        modules = [{
          home.username = "root";
          home.homeDirectory = "/root";
          home.stateVersion = "24.11";

          home.packages = with pkgs; [
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
    };
}
