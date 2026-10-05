# Shared build plumbing for isolated generated-source fixtures, never products.
{ pkgs }:
{ name, script }:
pkgs.stdenv.mkDerivation (
  {
    inherit name;
    src = ../../runtime;
    cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../../runtime/Cargo.lock; };
    nativeBuildInputs = [
      (import ../toolchain.nix { inherit pkgs; }).dev
      pkgs.rustPlatform.cargoSetupHook
    ]
    ++ pkgs.lib.optional pkgs.stdenv.hostPlatform.isDarwin pkgs.rust-bindgen-unwrapped;
    buildPhase = script;
    installPhase = ''touch "$out"'';
  }
  // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isDarwin {
    SDKROOT = "${pkgs.apple-sdk.sdkroot}";
  }
)
