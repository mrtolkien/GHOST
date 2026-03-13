{
  description = "Ghost — personal AI agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in {
      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system}; in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "ghost";
            version = self.shortRev or self.dirtyShortRev or "dev";
            src = self;
            cargoLock.lockFile = ./Cargo.lock;

            # Pass git hash to build.rs (no .git in the Nix store)
            GIT_COMMIT_HASH = self.shortRev or self.dirtyShortRev or "unknown";

            # Tests require CA certs / network; run in CI instead
            doCheck = false;

            nativeBuildInputs = with pkgs; [ pkg-config cmake perl ];
            buildInputs = with pkgs; [ openssl sqlite ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                pkgs.darwin.apple_sdk.frameworks.Security
                pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
              ];
          };
        }
      );
    };
}
