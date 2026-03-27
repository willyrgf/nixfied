{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  runtimeSource = builtins.readFile ../../nixfied/framework/runtime/executor-runtime.nix;
  sharedRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/shared-runtime-lib.nix;
  runtimeMetadataSource = builtins.readFile ../../nixfied/framework/runtime/runtime-metadata.nix;
  hasSharedRuntimeInfix =
    pattern: pkgs.lib.hasInfix pattern source || pkgs.lib.hasInfix pattern sharedRuntimeSource;
in
assert hasSharedRuntimeInfix "compute_run_id() {";
assert pkgs.lib.hasInfix "compute_attempt_id() {" source;
assert hasSharedRuntimeInfix "kernel_run_id_envelope() {";
assert hasSharedRuntimeInfix "nixfied-kernel run-id envelope";
assert hasSharedRuntimeInfix "run_id_pass_through_env_file() {";
assert hasSharedRuntimeInfix "write_name_value_tsv_file() {";
assert pkgs.lib.hasInfix "ERROR: usage: run-task <task-id> [-- ...]" source;
assert pkgs.lib.hasInfix "ERROR: usage: run-workflow <workflow-id> [-- ...]" source;
assert pkgs.lib.hasInfix
  "ERROR: usage: run-task-leaf <task-id> <workflow-id> <selected-services-csv> [-- ...]"
  source;
assert pkgs.lib.hasInfix
  "ERROR: usage: run-service-set-phase <service-set-id> <operation> <selected-services-csv>"
  source;
assert pkgs.lib.hasInfix "runtimeMetadataShell = import ./runtime-metadata.nix" source;
assert pkgs.lib.hasInfix "\${runtimeMetadataShell}" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow run" source;
assert pkgs.lib.hasInfix "run_workflow_kernel_impl() {" source;
assert pkgs.lib.hasInfix "run_task_leaf() {" source;
assert pkgs.lib.hasInfix "run_service_set_phase() {" source;
assert pkgs.lib.hasInfix "workflowTaskAdapter = pkgs.writeShellScript" source;
assert pkgs.lib.hasInfix "workflowServiceSetAdapter = pkgs.writeShellScript" source;
assert pkgs.lib.hasInfix "\"$MODEL_FILE\"" source;
assert pkgs.lib.hasInfix "nixfied-kernel task execution-order" source;
assert pkgs.lib.hasInfix "run_task_with_deps() {" source;
assert pkgs.lib.hasInfix "retrying attempt=" source;
assert pkgs.lib.hasInfix "extract_machine_output_args \"''\${filtered_args[@]}\"" source;
assert pkgs.lib.hasInfix "write_text_file_atomic \"$MACHINE_RUN_ID_FILE\" \"$run_id\"" source;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary render-human" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary compose" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary collect-steps" source;
assert pkgs.lib.hasInfix "workflowSummaryPlanFile = pkgs.writeText" source;
assert pkgs.lib.hasInfix "LAST_WORKFLOW_SUMMARY_PASSED_COUNT" source;
assert pkgs.lib.hasInfix "workflow_collect_steps() {" source;
assert pkgs.lib.hasInfix "print_workflow_summary_report() {" source;
assert pkgs.lib.hasInfix "INFO: runId=$run_id passed=$passed failed=$failed canceled=$canceled"
  source;
assert pkgs.lib.hasInfix "INFO: task context runId=" source;
assert pkgs.lib.hasInfix "run_task_hooks() {" source;
assert pkgs.lib.hasInfix "INFO: hook $phase $hook_id start" source;
assert pkgs.lib.hasInfix "ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code" source;
assert pkgs.lib.hasInfix "runtime_plan_shell=\"$(task_runtime_plan_shell \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "command=\"$(task_runner_command \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "package_path=\"$(task_runner_package \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "done < <(task_hook_ids \"$task_id\" \"$phase\")" source;
assert pkgs.lib.hasInfix "workflow_write_summary \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_logging_level_default \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_parallel_enabled \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_resolve_mode_id \"$workflow_id\" \"$mode_override\"" source;
assert pkgs.lib.hasInfix
  "workflow_simple_shorthand_exists_for_family \"$workflow_id\" \"$shorthand_mode\""
  source;
assert pkgs.lib.hasInfix "workflow_unit_name() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_task_id() {" runtimeSource;
assert pkgs.lib.hasInfix "normalize_run_artifacts_dir() {" runtimeSource;
assert pkgs.lib.hasInfix "task_runtime_plan_shell() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "task_hook_runtime_plan_shell() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "workflow_parallel_enabled() {" runtimeMetadataSource;
assert (!pkgs.lib.hasInfix "workflowModesShell = import ./workflow-modes.nix" source);
assert (!pkgs.lib.hasInfix "nixfied-kernel workflow serial-init" source);
assert (!pkgs.lib.hasInfix "nixfied-kernel workflow serial-step" source);
assert (!pkgs.lib.hasInfix "nixfied-kernel workflow parallel-init" source);
assert (!pkgs.lib.hasInfix "nixfied-kernel workflow parallel-step" source);
assert (!pkgs.lib.hasInfix "workflowSchedulerPlanFile = pkgs.writeText" source);
assert (!pkgs.lib.hasInfix "taskDependencyPlanFile = pkgs.writeText" source);
assert (!pkgs.lib.hasInfix "run_workflow_phase \"$run_id\"" source);
assert (!pkgs.lib.hasInfix "run_workflow_phase_tasks \"$run_id\"" source);
assert (!pkgs.lib.hasInfix "run_workflow_phase_service_sets \"$run_id\"" source);
pkgs.runCommand "executor-contract" { } ''
  echo "OK: executor is manifest-backed and delegates workflow driving to one kernel run command" > "$out"
''
