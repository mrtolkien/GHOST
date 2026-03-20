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

          # Full source: Rust + non-Rust files needed by build.rs / include_str!
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

          # Deps-only source: only Cargo manifests, lockfile, and build.rs.
          # .rs source files are excluded so code edits don't change the hash.
          # The crate version is pinned to "0.0.0" so release-please bumps
          # don't bust the cache either. mkDummySrc handles creating .rs stubs
          # internally when buildDepsOnly consumes this.
          depsSrc = let
            cargoOnly = pkgs.lib.cleanSourceWith {
              src = self;
              filter = path: type:
                let base = builtins.baseNameOf path; in
                type == "directory"
                || base == "Cargo.toml"
                || base == "Cargo.lock"
                || base == "build.rs"
                || (pkgs.lib.hasInfix ".cargo" path);
            };
          in pkgs.runCommand "ghost-deps-src" {} ''
            cp -r ${cargoOnly} $out
            chmod -R u+w $out
            # Pin crate version so release-please bumps don't change the hash
            ${pkgs.gnused}/bin/sed -i '0,/^version = ".*"/s//version = "0.0.0"/' $out/Cargo.toml
            ${pkgs.gnused}/bin/sed -i '/^name = "ghost"$/{n;s/^version = ".*"/version = "0.0.0"/;}' $out/Cargo.lock
            # Create stub source files — cargo needs at least lib.rs or main.rs
            mkdir -p $out/src
            echo "" > $out/src/lib.rs
            echo "fn main() {}" > $out/src/main.rs
          '';

          commonArgs = {
            pname = "ghost";
            version = self.shortRev or self.dirtyShortRev or "dev";
            inherit src;
            strictDeps = true;

            GIT_COMMIT_HASH = self.shortRev or self.dirtyShortRev or "unknown";

            nativeBuildInputs = with pkgs; [ pkg-config cmake perl ];
            buildInputs = with pkgs; [ openssl sqlite ];
          };

          # Dependencies-only derivation — cached as long as Cargo.lock is stable.
          # Uses depsSrc (Cargo files only) so asset/doc changes don't bust the cache.
          # version and GIT_COMMIT_HASH are pinned to constants so the derivation
          # hash stays the same across commits (they change in commonArgs via shortRev).
          cargoArtifacts = craneLib.buildDepsOnly (commonArgs // {
            pname = "ghost-deps";
            version = "0.0.0";
            GIT_COMMIT_HASH = "deps";
            src = depsSrc;
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
