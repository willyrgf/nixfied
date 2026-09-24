# Compilation validates only the manifest's own structural declarations.
# Whole-inventory and presentation validation remain release-check consumers.
{ lib }:
let
  manifest = import ./manifest.nix { inherit lib; };
in
import ./structure.nix { inherit lib; } {
  inherit (manifest) records vocabularies;
  inventory = import ./inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-manifest/capability.txt
  );
}
