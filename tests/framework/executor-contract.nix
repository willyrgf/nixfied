{ pkgs }:
let
  source = builtins.readFile ../../nixfied/runner/executor.nix;
in
assert pkgs.lib.hasInfix "compute_run_id() {" source;
assert pkgs.lib.hasInfix "ERROR: usage: run-task <task-id> [-- ...]" source;
assert pkgs.lib.hasInfix "ERROR: usage: run-workflow <workflow-id> [-- ...]" source;
assert pkgs.lib.hasInfix "ERROR: unknown workflow '$workflow_id'" source;
assert pkgs.lib.hasInfix "INFO: runId=$run_id passed=$passed failed=$failed canceled=$canceled"
  source;
assert pkgs.lib.hasInfix "run_workflow_parallel_impl()" source;
assert pkgs.lib.hasInfix "wait -n -p done_pid" source;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_PARALLEL" source;
assert pkgs.lib.hasInfix ".execution.parallel // false" source;
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" source;
assert pkgs.lib.hasInfix "if .postRun.alwaysRun == null then true else .postRun.alwaysRun end" source;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" source;
assert pkgs.lib.hasInfix "summary_json=$summary_file" source;
assert pkgs.lib.hasInfix "run_task_hooks() {" source;
assert pkgs.lib.hasInfix "INFO: hook $phase $hook_id start" source;
assert pkgs.lib.hasInfix "ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code" source;
assert pkgs.lib.hasInfix "defines runtime hooks but runner type '$runner_type' is unsupported"
  source;
pkgs.runCommand "executor-contract" { } ''
  echo "OK: executor contract markers are stable" > "$out"
''
