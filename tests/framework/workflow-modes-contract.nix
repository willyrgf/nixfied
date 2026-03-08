{ pkgs }:
let
  source = builtins.readFile ../../nixfied/runner/workflow-modes.nix;
in
assert pkgs.lib.hasInfix "workflow_id_exists() {" source;
assert pkgs.lib.hasInfix "workflow_mode_name() {" source;
assert pkgs.lib.hasInfix "workflow_artifacts_root() {" source;
assert pkgs.lib.hasInfix "workflow_ephemeral_flag() {" source;
assert pkgs.lib.hasInfix "workflow_fail_fast() {" source;
assert pkgs.lib.hasInfix "workflow_parallel_enabled() {" source;
assert pkgs.lib.hasInfix "workflow_write_summary() {" source;
assert pkgs.lib.hasInfix "task_descriptor_exists() {" source;
assert pkgs.lib.hasInfix "task_arg_parser() {" source;
assert pkgs.lib.hasInfix "task_arg_long_kind() {" source;
assert pkgs.lib.hasInfix "task_runner_workflow_id() {" source;
assert (! pkgs.lib.hasInfix "$MODEL_FILE" source);
assert (! pkgs.lib.hasInfix "/bin/jq" source);
pkgs.runCommand "workflow-modes-contract" { } ''
  echo "OK: workflow/task runner descriptors are compiled" > "$out"
''
