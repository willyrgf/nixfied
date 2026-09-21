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
        decode = {
          kind = "Required";
        };
        rustEncode = "Present";
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
    recoveryTopics = topics;
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
  # Exact edits at the current native owner, with drift rejection. This is test
  # orchestration, not a syntax backend or a generated production parser.
  replaceOnce =
    before: after: text:
    assert lib.assertMsg (
      builtins.length (lib.splitString before text) == 2
    ) "native maintenance fixture insertion drifted";
    lib.replaceStrings [ before ] [ after ] text;
  runtime = lib.pipe (builtins.readFile ../../runtime/crates/nixfied-runtime/src/main.rs) [
    (replaceOnce "    let mut task = RUN_TASK_INITIAL.map(str::to_string);" "    let mut budget_ms = RUN_BUDGET_MS_INITIAL;\n    let mut task = RUN_TASK_INITIAL.map(str::to_string);")
    (replaceOnce "            RUN_OUTPUT => {" ''
      RUN_BUDGET_MS => {
          index += 1;
          let value = args.get(index).ok_or_else(|| RuntimeError::new(
              nixfied_runtime::ErrorCode::ModelAdmission, "missing fixture budget"))?;
          budget_ms = Some(value.parse::<RunBudgetMsValue>().map_err(|_| RuntimeError::new(
              nixfied_runtime::ErrorCode::ModelAdmission, "invalid fixture budget"))?);
      }
      RUN_OUTPUT => {
    '')
    (replaceOnce "struct ParsedRunOptions {" "struct ParsedRunOptions {\n    budget_ms: Option<RunBudgetMsValue>,")
    (replaceOnce "    Ok(ParsedRunOptions {" "    Ok(ParsedRunOptions {\n        budget_ms,")
    (replaceOnce "            timeout_ms: self.timeout_ms," "            timeout_ms: self.budget_ms.unwrap_or(self.timeout_ms),")
  ];
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
in
assert rejects (construct input);
assert rejects (construct (input // { fixtureEnabled = null; }));
assert
  builtins.toJSON (construct (input // { fixtureEnabled = true; }))
  == ''{"defaultOutput":"summary","fixtureEnabled":true,"kind":"composite","serviceLifetime":"run-scoped","servicesRequired":[]}'';
assert (construct (input // { fixtureEnabled = false; })).fixtureEnabled == false;
assert lib.hasInfix "--budget-ms <operation-budget-milliseconds>" syntax.help.run;
pkgs.stdenv.mkDerivation {
  name = "nixfied-maintenance-exercises";
  src = ../../runtime;
  cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../../runtime/Cargo.lock; };
  nativeBuildInputs = [
    toolchain
    pkgs.rustPlatform.cargoSetupHook
  ];
  buildPhase = ''
    ${reference}/bin/nixfied-docs api record local/FixtureTaskSpec > wire-reference.txt
    grep -Fq 'fixtureEnabled' wire-reference.txt
    grep -Fq 'Decode: Required' wire-reference.txt
    ${reference}/bin/nixfied-docs api command run > command-reference.txt
    grep -Fq -- '--budget-ms <operation-budget-milliseconds>' command-reference.txt
    grep -Fq 'Domain: Unsigned64' command-reference.txt
    grep -Fq 'Fixture operation budget' command-reference.txt
    cp ${generated}/crates/nixfied-model/src/generated/types.rs crates/nixfied-model/src/generated/types.rs
    cp ${./maintenance-wire.rs} crates/nixfied-model/tests/maintenance_wire.rs
    cp ${generated}/crates/nixfied-runtime/src/generated/commands.rs crates/nixfied-runtime/src/generated/commands.rs
    cp ${pkgs.writeText "maintenance-main.rs" runtime} crates/nixfied-runtime/src/main.rs
    cat ${./maintenance-native.rs} >> crates/nixfied-runtime/src/main_tests.rs
    cargo test --offline --locked -p nixfied-model --test maintenance_wire
    cargo test --offline --locked -p nixfied-runtime --bin nixfied-runtime maintenance_
  '';
  installPhase = ''touch "$out"'';
}
