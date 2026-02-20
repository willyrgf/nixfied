{ pkgs }:
let
  orchestratorSource = builtins.readFile ../../nixfied/runner/orchestrator.nix;
  dispatcherSource = builtins.readFile ../../nixfied/runner/dispatcher.nix;
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
in
assert pkgs.lib.hasInfix "nixfied-orchestrator" orchestratorSource;
assert pkgs.lib.hasInfix "create_run_record() {" orchestratorSource;
assert pkgs.lib.hasInfix "update_run_state() {" orchestratorSource;
assert pkgs.lib.hasInfix "stop-all-runs" orchestratorSource;
assert pkgs.lib.hasInfix "validate_typed_task_args() {" orchestratorSource;
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" executorSource;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" executorSource;
assert pkgs.lib.hasInfix "NIXFIED_ORCHESTRATOR_RUN_ID" executorSource;
assert pkgs.lib.hasInfix "frameworkEphemeral = import ../.framework/ephemeral.nix" orchestratorSource;
assert pkgs.lib.hasInfix "EPHEMERAL_EXECUTOR_WRAPPER=" orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-task" orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-workflow" orchestratorSource;
assert pkgs.lib.hasInfix "orchestratorProgram =" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-task" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-workflow" dispatcherSource;
pkgs.runCommand "orchestrator-lifecycle-contract" { } ''
  echo "OK: orchestrator lifecycle contracts are stable" > "$out"
''
