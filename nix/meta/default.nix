# Cohesive declaration bundles; individual records stay in their owning bundle.
{ lib }:
let
  bundles = map (file: import file { inherit lib; }) [
    ./manifest.nix
    ./outputs.nix
  ];
in
import ./structure.nix { inherit lib; } {
  records = lib.concatMap (bundle: bundle.records) bundles;
  vocabularies = lib.concatMap (bundle: bundle.vocabularies) bundles;
  contextTopics = builtins.attrNames (import ../docs/topics.nix);
  inventory = import ./inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-manifest/capability.txt
  );
}
