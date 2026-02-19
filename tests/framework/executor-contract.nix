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
pkgs.runCommand "executor-contract" { } ''
  echo "OK: executor contract markers are stable" > "$out"
''
