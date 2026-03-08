{ lib, stdenv, fetchurl, autoPatchelfHook, glibc }:

let
  version = "0.1.0";

  sources = {
    x86_64-linux = {
      url = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-x86_64-linux";
      hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    };
    aarch64-linux = {
      url = "https://github.com/mrtolkien/ghost/releases/download/v${version}/ghost-aarch64-linux";
      hash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    };
  };

  src = sources.${stdenv.hostPlatform.system}
    or (throw "Unsupported system: ${stdenv.hostPlatform.system}");
in
stdenv.mkDerivation {
  pname = "ghost";
  inherit version;

  src = fetchurl {
    inherit (src) url hash;
  };

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
