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

          # Deps-only source: Cargo files with the root crate version pinned
          # to "0.0.0" so release-please version bumps don't bust the cache.
          # Only actual dependency changes (Cargo.lock entries) trigger a rebuild.
          depsSrc = let
            cleaned = craneLib.cleanCargoSource self;
          in pkgs.runCommand "ghost-deps-src" {} ''
            cp -r ${cleaned} $out
            chmod -R u+w $out
            # Pin root crate version so release-please bumps don't bust deps cache.
            ${pkgs.gnused}/bin/sed -i '0,/^version = ".*"/s//version = "0.0.0"/' $out/Cargo.toml
            # Rewrite ONLY the ghost entry in Cargo.lock (name = "ghost" is followed by version =)
            ${pkgs.gnused}/bin/sed -i '/^name = "ghost"$/{n;s/^version = ".*"/version = "0.0.0"/;}' $out/Cargo.lock
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
