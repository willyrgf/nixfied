{ pkgs }:
let
  shellContractSource = builtins.readFile ../../nixfied/framework/runtime/helpers/shell-contract.nix;
  buildersSource = builtins.readFile ../../nixfied/framework/runtime/helpers/builders.nix;
  ephemeralSource = builtins.readFile ../../nixfied/framework/runtime/ephemeral.nix;
in
assert pkgs.lib.hasInfix "mkContractRuntime =" shellContractSource;
assert pkgs.lib.hasInfix "_nixfied_contract_load_runtime_plan() {" shellContractSource;
assert pkgs.lib.hasInfix "builtins.toJSON" shellContractSource;
assert pkgs.lib.hasInfix "validate-input" shellContractSource;
assert pkgs.lib.hasInfix "validate-exit" shellContractSource;
assert pkgs.lib.hasInfix "_nixfied_contract_eval_exports() {" shellContractSource;
assert (!pkgs.lib.hasInfix "_nixfied_contract_source_exports() {" shellContractSource);
assert (!pkgs.lib.hasInfix "mktemp \"''${"TMPDIR:-/tmp"}/nixfied-contract-env" shellContractSource);
assert (
  !pkgs.lib.hasInfix "mktemp \"''${"TMPDIR:-/tmp"}/nixfied-contract-args" shellContractSource
);
assert !(pkgs.lib.hasInfix "NIXFIED_CONTRACT_ENV_NAMES" shellContractSource);
assert !(pkgs.lib.hasInfix "NIXFIED_CONTRACT_ARG_BY_LONG" shellContractSource);
assert !(pkgs.lib.hasInfix "NIXFIED_CONTRACT_FAILURE_CODE" shellContractSource);
assert !(pkgs.lib.hasInfix "(.env // [])[]" shellContractSource);
assert !(pkgs.lib.hasInfix "(.args // [])[]" shellContractSource);
assert !(pkgs.lib.hasInfix "any(.value == $code)" shellContractSource);
assert pkgs.lib.hasInfix "NIXFIED_COMMAND_API_RUNTIME" buildersSource;
assert pkgs.lib.hasInfix "NIXFIED_COMMAND_API_RUNTIME" ephemeralSource;
pkgs.runCommand "shell-contract-contract" { } ''
  echo "OK: shell contract runtime uses direct kernel export streams" > "$out"
''
