# Independent whole-inventory coverage audit, not an executable behavior registry.
{ lib }:
let
  structure = import ../meta/default.nix { inherit lib; };
  syntax = import ../meta/command-default.nix { inherit lib; };
  inventory = import ../meta/inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-manifest/capability.txt
  );
  topics = import ../docs/topics.nix;
  records = map (record: lib.replaceStrings [ "/" ] [ " " ] record.id) (
    builtins.filter (record: !lib.hasPrefix "local/" record.id) structure.records
  );
  vocabularies = map (vocabulary: vocabulary.coordinate) structure.vocabularies;
  # These deliberately retain native owners. The matching routing table is in
  # DEVELOPMENT's delivery audit and the packaged topics contain those owners.
  native = {
    manifest-version = "manifest";
    substitution = "placeholders";
    surface = "commands";
    surface-help = "commands";
    "output-schema run-task-output" = "outputs";
    "output-schema run-summary-text" = "outputs";
    "output-schema run-error-summary-text" = "outputs";
    endpoint-acquisition = "runtime";
    endpoint-reuse = "runtime";
    lease-authority = "runtime";
    escape-settlement = "runtime";
    escaped-port-reconciliation = "runtime";
  };
  covered = records ++ vocabularies ++ builtins.attrNames native;
in
assert lib.unique covered == covered;
assert builtins.sort builtins.lessThan covered == builtins.attrNames inventory;
assert builtins.all (topic: topics ? ${topic}) (builtins.attrValues native);
assert syntax.runtimeCommands == inventory.surface;
assert builtins.length structure.records == 48;
assert builtins.length structure.vocabularies == 23;
assert
  builtins.sort builtins.lessThan (
    map (record: record.id) (
      builtins.filter (record: lib.hasPrefix "local/" record.id) structure.records
    )
  ) == [
    "local/CheckOutput"
    "local/CleanupOutcome"
    "local/DownReport"
    "local/RegistryIdentityDiagnostic"
  ];
true
