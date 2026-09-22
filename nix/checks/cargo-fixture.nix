# Shared build plumbing for isolated generated-source fixtures, never products.
{ pkgs }:
{ name, script }:
pkgs.stdenv.mkDerivation {
  inherit name;
  src = ../../runtime;
  cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../../runtime/Cargo.lock; };
  nativeBuildInputs = [
    (import ../toolchain.nix { inherit pkgs; }).dev
    pkgs.rustPlatform.cargoSetupHook
  ];
  buildPhase = script;
  installPhase = ''touch "$out"'';
}
