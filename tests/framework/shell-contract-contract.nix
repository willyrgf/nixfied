{ pkgs }:
let
  shellContractSource = builtins.readFile ../../nixfied/.framework/lib/shell-contract.nix;
  buildersSource = builtins.readFile ../../nixfied/.framework/lib/builders.nix;
  ephemeralSource = builtins.readFile ../../nixfied/framework/runtime/ephemeral.nix;
in
assert pkgs.lib.hasInfix "mkContractRuntime =" shellContractSource;
assert pkgs.lib.hasInfix "_nixfied_contract_load_runtime_plan() {" shellContractSource;
assert pkgs.lib.hasInfix "NIXFIED_CONTRACT_ENV_NAMES" shellContractSource;
assert pkgs.lib.hasInfix "NIXFIED_CONTRACT_ARG_BY_LONG" shellContractSource;
assert pkgs.lib.hasInfix "NIXFIED_CONTRACT_FAILURE_CODE" shellContractSource;
assert !(pkgs.lib.hasInfix "(.env // [])[]" shellContractSource);
assert !(pkgs.lib.hasInfix "(.args // [])[]" shellContractSource);
assert !(pkgs.lib.hasInfix "any(.value == $code)" shellContractSource);
assert pkgs.lib.hasInfix "NIXFIED_APP_CONTRACT_RUNTIME" buildersSource;
assert pkgs.lib.hasInfix "NIXFIED_APP_CONTRACT_RUNTIME" ephemeralSource;
pkgs.runCommand "shell-contract-contract" { } ''
  echo "OK: shell contract runtime markers are stable" > "$out"
''
