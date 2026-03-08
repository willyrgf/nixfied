{ pkgs }:
let
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
  runtimeSource = builtins.readFile ../../nixfied/runner/executor-runtime.nix;
in
assert pkgs.lib.hasInfix "executorRuntimeShell = import ./executor-runtime.nix" executorSource;
assert pkgs.lib.hasInfix "\${executorRuntimeShell}" executorSource;
assert (!pkgs.lib.hasInfix "extract_machine_output_args() {" executorSource);
assert (!pkgs.lib.hasInfix "write_text_file_atomic() {" executorSource);
assert (!pkgs.lib.hasInfix "ensure_run_artifacts_dir() {" executorSource);
assert (!pkgs.lib.hasInfix "emit_workflow_result_json() {" executorSource);
assert pkgs.lib.hasInfix "ensure_run_artifacts_dir() {" runtimeSource;
assert pkgs.lib.hasInfix "emit_workflow_result_json() {" runtimeSource;
assert pkgs.lib.hasInfix "extract_logging_override_args() {" runtimeSource;
assert pkgs.lib.hasInfix "extract_machine_output_args() {" runtimeSource;
assert pkgs.lib.hasInfix "write_text_file_atomic() {" runtimeSource;
assert pkgs.lib.hasInfix "copy_file_atomic() {" runtimeSource;
assert pkgs.lib.hasInfix "NIXFIED_JSON_OUTPUT_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "--run-id-file" runtimeSource;
assert pkgs.lib.hasInfix "--summary-file" runtimeSource;
assert pkgs.lib.hasInfix "workflow_artifacts_root \"$workflow_id\"" runtimeSource;
pkgs.runCommand "executor-runtime-contract" { } ''
  echo "OK: executor runtime helpers are split and stable" > "$out"
''
