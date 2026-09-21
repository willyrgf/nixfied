# Invoked only in the gate's copied source with StdinPolicy changed to null/pipe.
{ pkgs }:
let
  inherit (pkgs) lib;
  options = import ../modules/invocation.nix {
    inherit lib;
    positiveInt = lib.types.ints.positive;
  };
  generated = import ../meta/generated.nix { inherit pkgs; };
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
in
assert options.stdin.type.check "null";
assert options.stdin.type.check "pipe";
assert !options.stdin.type.check "inherit";
assert !options.stdin.type.check "other";
pkgs.stdenv.mkDerivation {
  name = "nixfied-inventory-mutation-check";
  src = ../../runtime;
  cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../../runtime/Cargo.lock; };
  nativeBuildInputs = [
    toolchain
    pkgs.rustPlatform.cargoSetupHook
  ];
  buildPhase = ''
    # Test fixture only: product builds never overlay regenerated sources.
    cp ${generated}/crates/nixfied-model/src/generated/types.rs crates/nixfied-model/src/generated/types.rs
    cp ${./inventory-mutation.rs} crates/nixfied-model/tests/inventory_mutation.rs
    cargo test --offline --locked -p nixfied-model --test inventory_mutation
  '';
  installPhase = ''touch "$out"'';
}
