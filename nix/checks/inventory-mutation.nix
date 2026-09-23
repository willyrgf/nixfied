# Invoked only in the gate's copied source with StdinPolicy changed to null/pipe.
{ pkgs }:
let
  inherit (pkgs) lib;
  options = import ../modules/invocation.nix {
    inherit lib;
    positiveInt = lib.types.ints.positive;
  };
  generated = import ../meta/generated.nix { inherit pkgs; };
in
assert options.stdin.type.check "null";
assert options.stdin.type.check "pipe";
assert !options.stdin.type.check "inherit";
assert !options.stdin.type.check "other";
import ./cargo-fixture.nix { inherit pkgs; } {
  name = "nixfied-inventory-mutation-check";
  script = ''
    # Test fixture only: product builds never overlay regenerated sources.
    cp ${generated}/crates/nixfied-manifest/src/generated/types.rs crates/nixfied-manifest/src/generated/types.rs
    cp ${./inventory-mutation.rs} crates/nixfied-manifest/tests/inventory_mutation.rs
    cargo test --offline --locked -p nixfied-manifest --test inventory_mutation
  '';
}
