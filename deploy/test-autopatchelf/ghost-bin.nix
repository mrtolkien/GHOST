# Nix derivation that wraps a pre-built ghost binary using autoPatchelfHook.
#
# autoPatchelfHook automatically discovers needed shared libraries from
# buildInputs and patches the ELF interpreter + rpath. This replaces the
# manual patchelf invocations in the production Dockerfile.
#
# Usage (from inside a Dockerfile with nix available):
#   nix-build ghost-bin.nix --arg ghostBinary /path/to/ghost
#
# The result will be in /nix/store/.../bin/ghost with correct interpreter
# and rpath pointing into /nix/store/...-glibc-.../lib etc.

{ pkgs ? import <nixpkgs> { }
, ghostBinary ? /usr/local/bin/ghost
}:

pkgs.stdenv.mkDerivation {
  pname = "ghost-bin";
  version = "0.0.0";

  # No source to unpack — we're wrapping an existing binary
  dontUnpack = true;

  # autoPatchelfHook scans ELF binaries in $out and patches them
  nativeBuildInputs = [ pkgs.autoPatchelfHook ];

  # These provide the shared libraries that autoPatchelfHook will resolve:
  # - glibc: libc.so.6, libm.so.6, ld-linux-x86-64.so.2
  # - gcc.cc.lib (libgcc): libgcc_s.so.1
  buildInputs = [
    pkgs.glibc
    pkgs.gcc.cc.lib
  ];

  installPhase = ''
    mkdir -p $out/bin
    cp ${ghostBinary} $out/bin/ghost
    chmod +x $out/bin/ghost
  '';

  meta = {
    description = "Pre-built ghost binary with patched ELF interpreter and rpath";
    platforms = [ "x86_64-linux" ];
  };
}
