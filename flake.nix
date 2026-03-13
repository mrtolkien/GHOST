{
  description = "Ghost — personal AI agent";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, crane }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          craneLib = crane.mkLib pkgs;

          # Include Rust sources + non-Rust files needed by build.rs / include_str!
          src = pkgs.lib.cleanSourceWith {
            src = self;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || (builtins.match ".*prompts/.*" path != null)
              || (builtins.match ".*assets/.*" path != null)
              || (builtins.match ".*docs/src/content/.*" path != null)
              || (builtins.match ".*tests/fixtures/.*" path != null)
              || (builtins.match ".*migrations/.*" path != null);
          };

          commonArgs = {
            pname = "ghost";
            version = self.shortRev or self.dirtyShortRev or "dev";
            inherit src;
            strictDeps = true;

            GIT_COMMIT_HASH = self.shortRev or self.dirtyShortRev or "unknown";

            nativeBuildInputs = with pkgs; [ pkg-config cmake perl ];
            buildInputs = with pkgs; [ openssl sqlite ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                pkgs.darwin.apple_sdk.frameworks.Security
                pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
              ];
          };

          # Dependencies-only derivation — cached when Cargo.lock doesn't change
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "ghost-deps";
          });
        in {
          default = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
            doCheck = false;
          });
        }
      );
    };
}
