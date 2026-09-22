# Bounded fixture amendments: declaration + native consumer + independent oracle.
# Products are never overlaid; only this test's copied workspace is changed.
{ pkgs }:
let
  inherit (pkgs) lib;
  model = import ../meta/model.nix { inherit lib; };
  outputs = import ../meta/outputs.nix { inherit lib; };
  task = lib.findFirst (
    record: record.rust.name == "TaskSpec"
  ) (throw "TaskSpec fixture owner missing") model.records;
  fixtureTask = task // {
    identity = {
      kind = "Local";
      name = "FixtureTaskSpec";
    };
    rust = task.rust // {
      name = "FixtureTaskSpec";
    };
    description = "TaskSpec-shaped maintenance fixture with one added required boolean.";
    fields = task.fields ++ [
      {
        name = "fixtureEnabled";
        description = "Fixture-only native execution selection.";
        value = {
          kind = "Boolean";
        };
        presence = {
          kind = "Required";
        };
        nixEncode = "RequiredPresent";
        rust = {
          visibility = "pub";
          storage = "Direct";
        };
      }
    ];
  };
  inventory = import ../meta/inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
  );
  topics = builtins.attrNames (import ../docs/topics.nix);
  structure = import ../meta/structure.nix { inherit lib; } {
    inherit inventory;
    records = model.records ++ outputs.records ++ [ fixtureTask ];
    vocabularies = model.vocabularies ++ outputs.vocabularies;
    contextTopics = topics;
  };
  syntax = import ../meta/syntax.nix { inherit lib; } {
    inherit inventory structure topics;
    publications = import ../project-publications.nix { };
    declarations = map (
      command:
      if command.name != "run" then
        command
      else
        command
        // {
          arguments = command.arguments ++ [
            {
              id = "budgetMs";
              binding = "Command";
              token = "--budget-ms";
              valueDomain = {
                kind = "Unsigned";
                bits = 64;
              };
              initialValue = {
                kind = "Absent";
              };
              help = {
                kind = "Visible";
                metavar = "<operation-budget-milliseconds>";
                text = "Fixture operation budget";
              };
            }
          ];
        }
    ) (import ../meta/commands.nix { inherit lib structure; });
  };
  files = (import ../meta/rust.nix { inherit lib; } structure) // {
    "crates/nixfied-runtime/src/generated/commands.rs" =
      (import ../meta/syntax-project.nix { inherit lib structure; } syntax).rust
        inventory.surface;
  };
  generated = import ../meta/generated.nix { inherit pkgs files; };
  reference = import ../docs/reference.nix {
    inherit
      lib
      pkgs
      structure
      syntax
      ;
    system = pkgs.stdenv.hostPlatform.system;
    options = [ ];
    publications = [ ];
    # This structural fixture has no publication/option inventory to select.
    topics = builtins.mapAttrs (_: topic: topic // { select = [ ]; }) (import ../docs/topics.nix);
    source = {
      path = "maintenance-fixture";
    };
  };
  construct = structure.constructors."local/FixtureTaskSpec";
  input = {
    kind = "composite";
    defaultOutput = "summary";
    serviceLifetime = "run-scoped";
    servicesRequired = [ ];
  };
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;

in
assert rejects (construct input);
assert rejects (construct (input // { fixtureEnabled = null; }));
assert
  builtins.toJSON (construct (input // { fixtureEnabled = true; }))
  == ''{"defaultOutput":"summary","fixtureEnabled":true,"kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
assert (construct (input // { fixtureEnabled = false; })).fixtureEnabled == false;
assert lib.hasInfix "--budget-ms <operation-budget-milliseconds>" syntax.help.run;
import ./cargo-fixture.nix { inherit pkgs; } {
  name = "nixfied-maintenance-exercises";
  script = ''
    ${reference}/bin/nixfied-docs api record local/FixtureTaskSpec > wire-reference.txt
    grep -Fq 'fixtureEnabled' wire-reference.txt
    grep -Fq 'Decode: Required' wire-reference.txt
    ${reference}/bin/nixfied-docs api command run > command-reference.txt
    grep -Fq -- '--budget-ms <operation-budget-milliseconds>' command-reference.txt
    grep -Fq 'Domain: Unsigned64' command-reference.txt
    grep -Fq 'Fixture operation budget' command-reference.txt
    cp ${generated}/crates/nixfied-model/src/generated/types.rs crates/nixfied-model/src/generated/types.rs
    cp ${./maintenance-wire.rs} crates/nixfied-model/tests/maintenance_wire.rs
    cp ${generated}/crates/nixfied-runtime/src/generated/commands.rs crates/nixfied-runtime/src/generated/maintenance_commands.rs
    cat ${./maintenance-native.rs} >> crates/nixfied-runtime/src/main_tests.rs
    cargo test --offline --locked -p nixfied-model --test maintenance_wire
    cargo test --offline --locked -p nixfied-runtime --bin nixfied-runtime maintenance_
  '';
}
