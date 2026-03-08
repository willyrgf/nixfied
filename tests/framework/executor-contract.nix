{ pkgs }:
let
  source = builtins.readFile ../../nixfied/runner/executor.nix;
in
assert pkgs.lib.hasInfix "compute_run_id() {" source;
assert pkgs.lib.hasInfix "ERROR: usage: run-task <task-id> [-- ...]" source;
assert pkgs.lib.hasInfix "ERROR: usage: run-workflow <workflow-id> [-- ...]" source;
assert pkgs.lib.hasInfix "ERROR: unknown workflow '$workflow_id'" source;
assert pkgs.lib.hasInfix "workflow_resolve_mode_id \"$workflow_id\" \"$mode_override\"" source;
assert pkgs.lib.hasInfix
  "workflow_simple_shorthand_exists_for_family \"$workflow_id\" \"$shorthand_mode\""
  source;
assert pkgs.lib.hasInfix "ERROR: $override_name must be an integer >= 1 (got '$override_value')"
  source;
assert pkgs.lib.hasInfix "INFO: runId=$run_id passed=$passed failed=$failed canceled=$canceled"
  source;
assert pkgs.lib.hasInfix "run_workflow_parallel_impl()" source;
assert pkgs.lib.hasInfix "wait -n -p done_pid" source;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_PARALLEL" source;
assert pkgs.lib.hasInfix "workflow_parallel_enabled \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_unit_records() {" source;
assert pkgs.lib.hasInfix "workflow_plan_records \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "retrying attempt=" source;
assert pkgs.lib.hasInfix "run_task_with_deps() {" source;
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" source;
assert pkgs.lib.hasInfix "executorRuntimeShell = import ./executor-runtime.nix" source;
assert pkgs.lib.hasInfix "\${executorRuntimeShell}" source;
assert pkgs.lib.hasInfix "extract_machine_output_args \"''\${filtered_args[@]}\"" source;
assert pkgs.lib.hasInfix "--json and --summary cannot be combined" source;
assert pkgs.lib.hasInfix "write_text_file_atomic \"$MACHINE_RUN_ID_FILE\" \"$run_id\"" source;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" source;
assert pkgs.lib.hasInfix "ensure_run_artifacts_dir \"$run_id\" \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "registry_events_snapshot" source;
assert pkgs.lib.hasInfix "summary_json=$summary_file" source;
assert pkgs.lib.hasInfix "--json" source;
assert pkgs.lib.hasInfix "emit_workflow_result_json \"$run_id\" \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "NIXFIED_JSON_OUTPUT_OVERRIDE" source;
assert pkgs.lib.hasInfix "workflow_steps_json() {" source;
assert pkgs.lib.hasInfix "print_workflow_summary_report() {" source;
assert pkgs.lib.hasInfix "NIXFIED_PARENT_WORKFLOW_ID" source;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_NESTED=1 run_workflow" source;
assert pkgs.lib.hasInfix "INFO: Time breakdown" source;
assert pkgs.lib.hasInfix "run_task_hooks() {" source;
assert pkgs.lib.hasInfix "INFO: hook $phase $hook_id start" source;
assert pkgs.lib.hasInfix "ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code" source;
assert pkgs.lib.hasInfix "hook_command=\"$(task_hook_command \"$task_id\" \"$phase\" \"$hook_id\")\"" source;
assert pkgs.lib.hasInfix "hook_runtime_json=\"$(task_hook_runtime_json \"$task_id\" \"$phase\" \"$hook_id\")\"" source;
assert pkgs.lib.hasInfix "runtime_json=\"$(task_runtime_json \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "command=\"$(task_runner_command \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "package_path=\"$(task_runner_package \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "done < <(task_hook_ids \"$task_id\" \"$phase\")" source;
assert pkgs.lib.hasInfix "done < <(task_needs \"$current_task\")" source;
assert pkgs.lib.hasInfix "done < <(task_soft_needs \"$current_task\")" source;
assert pkgs.lib.hasInfix "defines runtime hooks but runner type '$runner_type' is unsupported"
  source;
assert pkgs.lib.hasInfix "workflow_post_run_always \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_write_summary \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_logging_level_default \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_lock_policy \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_fail_fast \"$workflow_id\"" source;
assert (!pkgs.lib.hasInfix "extract_machine_output_args() {" source);
assert (!pkgs.lib.hasInfix ".stages as $stages" source);
assert (!pkgs.lib.hasInfix "task_json() {" source);
pkgs.runCommand "executor-contract" { } ''
  echo "OK: executor contract markers are stable" > "$out"
''
