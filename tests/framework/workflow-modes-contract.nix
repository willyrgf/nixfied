{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/workflow-modes.nix;
in
assert pkgs.lib.hasInfix "selectionIndex" source;
assert (!pkgs.lib.hasInfix "service-selection.nix" source);
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
assert pkgs.lib.hasInfix "mergeTaskRuntimeWithRunnerPackage =" source;
assert pkgs.lib.hasInfix "mergeHookRuntime =" source;
assert pkgs.lib.hasInfix "task_runtime_json() {" source;
assert pkgs.lib.hasInfix "task_produces_json() {" source;
assert pkgs.lib.hasInfix "task_needs() {" source;
assert pkgs.lib.hasInfix "task_soft_needs() {" source;
assert pkgs.lib.hasInfix "task_hook_count() {" source;
assert pkgs.lib.hasInfix "task_hook_ids() {" source;
assert pkgs.lib.hasInfix "task_hook_command() {" source;
assert pkgs.lib.hasInfix "task_hook_runtime_json() {" source;
assert pkgs.lib.hasInfix "taskHookCases = builtins.concatLists" source;
assert pkgs.lib.hasInfix "workflowPlanCases = map" source;
assert pkgs.lib.hasInfix "\"workflow-unit:\${workflowId}:\${unit.name}\"" source;
assert pkgs.lib.hasInfix "workflow_plan_records() {" source;
assert pkgs.lib.hasInfix "workflowPhaseTaskCases = builtins.concatLists" source;
assert pkgs.lib.hasInfix "workflow_phase_tasks() {" source;
assert pkgs.lib.hasInfix "workflow_unit_closure_selected_services() {" source;
assert (!pkgs.lib.hasInfix "$MODEL_FILE" source);
assert (!pkgs.lib.hasInfix "/bin/jq" source);
pkgs.runCommand "workflow-modes-contract" { } ''
  echo "OK: workflow/task runner descriptors are compiled" > "$out"
''
