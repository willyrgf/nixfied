{ pkgs }:
let
  executorSource = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  orchestratorSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator.nix;
  handoffSource = builtins.readFile ../../nixfied/framework/runtime/runtime-handoff.nix;
  commonSource = builtins.readFile ../../nixfied/framework/runtime/common-runtime.nix;
in
assert pkgs.lib.hasInfix "runtimeHandoffShell = import ./runtime-handoff.nix" executorSource;
assert pkgs.lib.hasInfix "\${runtimeHandoffShell}" executorSource;
assert pkgs.lib.hasInfix "runtimeHandoffShell =" orchestratorSource;
assert pkgs.lib.hasInfix "\${runtimeHandoffShell}" orchestratorSource;
assert (!pkgs.lib.hasInfix "extract_machine_output_args() {" executorSource);
assert (!pkgs.lib.hasInfix "write_text_file_atomic() {" executorSource);
assert (!pkgs.lib.hasInfix "ensure_run_artifacts_dir() {" executorSource);
assert (!pkgs.lib.hasInfix "emit_workflow_result_json() {" executorSource);
assert pkgs.lib.hasInfix "commonRuntimeShell = import ./common-runtime.nix" handoffSource;
assert pkgs.lib.hasInfix "\${commonRuntimeShell}" handoffSource;
assert pkgs.lib.hasInfix "task_handoff_ensure() {" handoffSource;
assert pkgs.lib.hasInfix "workflow_handoff_ensure() {" handoffSource;
assert pkgs.lib.hasInfix "task_handoff_use() {" handoffSource;
assert pkgs.lib.hasInfix "workflow_handoff_use() {" handoffSource;
assert pkgs.lib.hasInfix "nixfied-kernel task handoff" handoffSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow handoff" handoffSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow resolve-mode" handoffSource;
assert pkgs.lib.hasInfix "ensure_run_artifacts_dir() {" handoffSource;
assert pkgs.lib.hasInfix "extract_logging_override_args() {" handoffSource;
assert pkgs.lib.hasInfix "extract_machine_output_args() {" handoffSource;
assert pkgs.lib.hasInfix "--run-id-file" handoffSource;
assert pkgs.lib.hasInfix "--summary-file" handoffSource;
assert !(pkgs.lib.hasInfix "renderCaseReturn =" handoffSource);
assert !(pkgs.lib.hasInfix "renderCasePrintLines =" handoffSource);
assert !(pkgs.lib.hasInfix "workflow-unit:" handoffSource);
assert !(pkgs.lib.hasInfix "((model.compiled or { }).execution or { })" handoffSource);
assert !(pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" handoffSource);
assert pkgs.lib.hasInfix "valid_log_level() {" commonSource;
assert pkgs.lib.hasInfix "valid_output_mode() {" commonSource;
assert pkgs.lib.hasInfix "write_text_file_atomic() {" commonSource;
assert pkgs.lib.hasInfix "copy_file_atomic() {" commonSource;
assert (!pkgs.lib.hasInfix "json_escape_string() {" commonSource);
assert (!pkgs.lib.hasInfix "json_quote_string() {" commonSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" commonSource);
assert pkgs.lib.hasInfix "ERROR: failed to move temp file into '$target'" commonSource;
pkgs.runCommand "runtime-handoff-contract" { } ''
  echo "OK: runtime handoff is a cached kernel-backed loader instead of a shell case-table metadata owner" > "$out"
''
