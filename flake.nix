{
  description = "Ghost binary package";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    ghost-bin = {
      url = "https://github.com/mrtolkien/GHOST/releases/latest/download/ghost-bin.tar.gz";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, ghost-bin }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          arch = if system == "x86_64-linux" then "x86_64" else "aarch64";
        in
        {
          default = pkgs.stdenv.mkDerivation {
            pname = "ghost";
            version = "bin";
            dontUnpack = true;

            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            buildInputs = [
              pkgs.glibc
              pkgs.gcc-unwrapped.lib
            ];

            installPhase = ''
              mkdir -p $out/bin
              cp ${ghost-bin}/${arch}/ghost $out/bin/ghost
              chmod +x $out/bin/ghost
            '';
          };
        }
      );
    };
}
