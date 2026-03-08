{ pkgs }:
let
  orchestratorSource = builtins.readFile ../../nixfied/runner/orchestrator.nix;
  runtimeSource = builtins.readFile ../../nixfied/runner/orchestrator-runtime.nix;
in
assert pkgs.lib.hasInfix "orchestratorRuntimeShell = import ./orchestrator-runtime.nix"
  orchestratorSource;
assert pkgs.lib.hasInfix "\${orchestratorRuntimeShell}" orchestratorSource;
assert (!pkgs.lib.hasInfix "split_process_mode() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "validate_workflow_args() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "validate_typed_task_args() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "resolve_run_artifacts_dir() {" orchestratorSource);
assert pkgs.lib.hasInfix "split_process_mode() {" runtimeSource;
assert pkgs.lib.hasInfix "validate_workflow_args() {" runtimeSource;
assert pkgs.lib.hasInfix "validate_typed_task_args() {" runtimeSource;
assert pkgs.lib.hasInfix "resolve_run_artifacts_dir() {" runtimeSource;
assert pkgs.lib.hasInfix "ensure_artifacts_root() {" runtimeSource;
assert pkgs.lib.hasInfix "NIXFIED_JSON_OUTPUT_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "NIXFIED_RUN_ID_FILE_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "NIXFIED_SUMMARY_FILE_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "workflow_simple_shorthand_exists_for_family" runtimeSource;
assert pkgs.lib.hasInfix "task_arg_long_kind" runtimeSource;
pkgs.runCommand "orchestrator-runtime-contract" { } ''
  echo "OK: orchestrator runtime helpers are split and stable" > "$out"
''
