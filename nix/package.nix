{ lib, stdenv, autoPatchelfHook, glibc }:

let
  version = "0.2.0"; # x-release-please-version

  sources = {
    x86_64-linux = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-x86_64-linux";
    aarch64-linux = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-aarch64-linux";
  };

  url = sources.${stdenv.hostPlatform.system}
    or (throw "Unsupported system: ${stdenv.hostPlatform.system}");
in
stdenv.mkDerivation {
  pname = "ghost";
  inherit version;

  src = builtins.fetchurl url;

  dontUnpack = true;
  nativeBuildInputs = [ autoPatchelfHook ];
  buildInputs = [ glibc ];

  installPhase = ''
    install -Dm755 $src $out/bin/ghost
  '';

  meta = with lib; {
    description = "GHOST personal AI agent platform";
    platforms = [ "x86_64-linux" "aarch64-linux" ];
  };
}
