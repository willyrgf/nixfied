{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  runtimeSource = builtins.readFile ../../nixfied/framework/runtime/executor-runtime.nix;
in
assert pkgs.lib.hasInfix "compute_run_id() {" source;
assert pkgs.lib.hasInfix "compute_attempt_id() {" source;
assert pkgs.lib.hasInfix "kernel_run_id_envelope() {" source;
assert pkgs.lib.hasInfix "nixfied-kernel run-id envelope" source;
assert pkgs.lib.hasInfix "run_id_pass_through_env_file() {" source;
assert pkgs.lib.hasInfix "write_name_value_tsv_file() {" source;
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
assert pkgs.lib.hasInfix "nixfied-kernel workflow serial-init" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow serial-next" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow serial-transition" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow parallel-init" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow parallel-next" source;
assert pkgs.lib.hasInfix "nixfied-kernel workflow parallel-transition" source;
assert pkgs.lib.hasInfix "wait -n -p done_pid" source;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_PARALLEL" source;
assert pkgs.lib.hasInfix "workflow_parallel_enabled \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflowSchedulerPlanFile = pkgs.writeText" source;
assert pkgs.lib.hasInfix "workflow_unit_records() {" source;
assert pkgs.lib.hasInfix "workflow_plan_records \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_phase_tasks \"$workflow_id\" \"$phase_key\"" source;
assert pkgs.lib.hasInfix "workflow_unit_closure_selected_services \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "task_retry_backoff_values \"$task_id\"" source;
assert pkgs.lib.hasInfix "retrying attempt=" source;
assert pkgs.lib.hasInfix "run_task_with_deps() {" source;
assert pkgs.lib.hasInfix "nixfied-kernel task execution-order" source;
assert pkgs.lib.hasInfix "taskDependencyPlanFile = pkgs.writeText" source;
assert pkgs.lib.hasInfix "write_skipped_services_file() {" source;
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" source;
assert pkgs.lib.hasInfix "executorRuntimeShell = import ./executor-runtime.nix" source;
assert pkgs.lib.hasInfix "\${executorRuntimeShell}" source;
assert pkgs.lib.hasInfix "extract_machine_output_args \"''\${filtered_args[@]}\"" source;
assert pkgs.lib.hasInfix "write_text_file_atomic \"$MACHINE_RUN_ID_FILE\" \"$run_id\"" source;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" source;
assert pkgs.lib.hasInfix "LAST_WORKFLOW_ATTEMPT_ID" source;
assert pkgs.lib.hasInfix "ensure_run_artifacts_dir \"$run_id\" \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "registry_events_index_snapshot" source;
assert pkgs.lib.hasInfix "summary_json=$summary_file" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary render-human" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary compose" source;
assert pkgs.lib.hasInfix "nixfied-kernel summary collect-steps" source;
assert pkgs.lib.hasInfix "workflowSummaryPlanFile = pkgs.writeText" source;
assert pkgs.lib.hasInfix "LAST_WORKFLOW_SUMMARY_PASSED_COUNT" source;
assert pkgs.lib.hasInfix "workflow_collect_steps() {" source;
assert pkgs.lib.hasInfix "event_detail_mode_json() {" source;
assert pkgs.lib.hasInfix "kernel_event_detail() {" source;
assert pkgs.lib.hasInfix "nixfied-kernel event-detail render" source;
assert pkgs.lib.hasInfix "event_detail_reason_exit_code_json() {" source;
assert pkgs.lib.hasInfix "workflow_phase_service_set_failure_json() {" source;
assert pkgs.lib.hasInfix "WORKFLOW_LEAF_TASK_IDS_LINES" source;
assert pkgs.lib.hasInfix "NIXFIED_ATTEMPT_ID" source;
assert pkgs.lib.hasInfix "workflow_steps_json() {" source;
assert pkgs.lib.hasInfix "print_workflow_summary_report() {" source;
assert pkgs.lib.hasInfix "NIXFIED_PARENT_WORKFLOW_ID" source;
assert pkgs.lib.hasInfix "NIXFIED_TASK_ID" source;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_NESTED=1 run_workflow" source;
assert pkgs.lib.hasInfix "INFO: task context runId=" source;
assert pkgs.lib.hasInfix "run_task_hooks() {" source;
assert pkgs.lib.hasInfix "INFO: hook $phase $hook_id start" source;
assert pkgs.lib.hasInfix "ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code" source;
assert pkgs.lib.hasInfix
  "hook_command=\"$(task_hook_command \"$task_id\" \"$phase\" \"$hook_id\")\""
  source;
assert pkgs.lib.hasInfix
  "hook_runtime_plan_shell=\"$(task_hook_runtime_plan_shell \"$task_id\" \"$phase\" \"$hook_id\")\""
  source;
assert pkgs.lib.hasInfix "runtime_plan_shell=\"$(task_runtime_plan_shell \"$task_id\")\"" source;
assert (!pkgs.lib.hasInfix "hook_runtime_json=\"$(task_hook_runtime_json" source);
assert (!pkgs.lib.hasInfix "runtime_json=\"$(task_runtime_json" source);
assert pkgs.lib.hasInfix "command=\"$(task_runner_command \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "package_path=\"$(task_runner_package \"$task_id\")\"" source;
assert pkgs.lib.hasInfix "done < <(task_hook_ids \"$task_id\" \"$phase\")" source;
assert pkgs.lib.hasInfix "defines runtime hooks but runner type '$runner_type' is unsupported"
  source;
assert pkgs.lib.hasInfix "workflow_post_run_always \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_write_summary \"$workflow_id\"" source;
assert pkgs.lib.hasInfix "workflow_logging_level_default \"$workflow_id\"" source;
assert (!pkgs.lib.hasInfix "workflow_lock_policy \"$workflow_id\"" source);
assert pkgs.lib.hasInfix "workflow_fail_fast \"$workflow_id\"" source;
assert (!pkgs.lib.hasInfix "normalize_env_hash() {" source);
assert (!pkgs.lib.hasInfix "extract_machine_output_args() {" source);
assert (!pkgs.lib.hasInfix ".stages as $stages" source);
assert (!pkgs.lib.hasInfix "task_json() {" source);
assert (!pkgs.lib.hasInfix "workflow_json() {" source);
assert (!pkgs.lib.hasInfix ".payload.steps // []" source);
assert (!pkgs.lib.hasInfix ".payload.counts.passed // 0" source);
assert (!pkgs.lib.hasInfix "[.[] | select(.state == \"passed\")] | length" source);
assert (!pkgs.lib.hasInfix "[.[] | .name] | unique" source);
assert (!pkgs.lib.hasInfix "workflow_step_status() {" source);
assert (!pkgs.lib.hasInfix "workflow_step_records_tsv() {" source);
assert (!pkgs.lib.hasInfix "workflow_peak_workers() {" source);
assert (!pkgs.lib.hasInfix "mark_unit_canceled() {" source);
assert (!pkgs.lib.hasInfix "cancel_pending_dependents() {" source);
assert (!pkgs.lib.hasInfix "cancel_pending_units() {" source);
assert (!pkgs.lib.hasInfix "unit_has_lock_conflict() {" source);
assert (!pkgs.lib.hasInfix "assign_unit_locks() {" source);
assert (!pkgs.lib.hasInfix "release_unit_locks() {" source);
assert (!pkgs.lib.hasInfix "next_ready_unit() {" source);
assert (!pkgs.lib.hasInfix "start_unit() {" source);
assert (!pkgs.lib.hasInfix "cancel_running_units() {" source);
assert (!pkgs.lib.hasInfix "UNIT_NEEDS_LEFT" source);
assert (!pkgs.lib.hasInfix "UNIT_STATE" source);
assert (!pkgs.lib.hasInfix "UNIT_DEPENDENTS" source);
assert (!pkgs.lib.hasInfix "UNIT_LOCKS" source);
assert (!pkgs.lib.hasInfix "LOCK_OWNER" source);
assert (!pkgs.lib.hasInfix "blocked_tasks_by_dependency" source);
assert (!pkgs.lib.hasInfix "blocked_tasks_reason_by_dependency" source);
assert (!pkgs.lib.hasInfix "local -A visited_tasks" source);
assert (!pkgs.lib.hasInfix "local -A active_tasks" source);
assert (!pkgs.lib.hasInfix "done < <(task_needs \"$current_task\")" source);
assert (!pkgs.lib.hasInfix "done < <(task_soft_needs \"$current_task\")" source);
assert (!pkgs.lib.hasInfix "workflow_unit_missing_env_csv \"$unit_json\"" source);
assert (!pkgs.lib.hasInfix "workflow_unit_when_matches \"$unit_json\"" source);
assert (!pkgs.lib.hasInfix "workflow_unit_dependencies \"''\${UNIT_JSON[\$unit_name]}\"" source);
assert (!pkgs.lib.hasInfix "workflow_unit_produces_json \"''\${UNIT_JSON[\$done_unit]}\"" source);
assert (!pkgs.lib.hasInfix "summary.fields" source);
assert (!pkgs.lib.hasInfix "summary.steps.tsv" source);
assert (!pkgs.lib.hasInfix "json_quote_string" source);
assert (!pkgs.lib.hasInfix "json_string_or_null" source);
assert (!pkgs.lib.hasInfix "--json" source);
assert (!pkgs.lib.hasInfix "emit_workflow_result_json" source);
assert (!pkgs.lib.hasInfix "NIXFIED_JSON_OUTPUT_OVERRIDE" source);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert (!pkgs.lib.hasInfix "jq -r -s --arg runId" source);
assert (!pkgs.lib.hasInfix ".skipIfMissingEnv[]?" source);
assert (!pkgs.lib.hasInfix ".when.envPresent[]?" source);
assert (!pkgs.lib.hasInfix ".when.envEquals // {}" source);
assert pkgs.lib.hasInfix "workflow_unit_name() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_task_id() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_needs_count() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_dependencies() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_lock_list() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_produces_json() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_missing_env_csv() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_unit_when_matches() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow-unit:" runtimeSource;
assert pkgs.lib.hasInfix "normalize_run_artifacts_dir() {" runtimeSource;
pkgs.runCommand "executor-contract" { } ''
  echo "OK: executor contract markers are stable" > "$out"
''
