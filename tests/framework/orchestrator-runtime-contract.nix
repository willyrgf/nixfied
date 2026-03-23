{ pkgs }:
let
  orchestratorSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator.nix;
  controlSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator-control.nix;
  runtimeSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator-runtime.nix;
  commonSource = builtins.readFile ../../nixfied/framework/runtime/common-runtime.nix;
in
assert pkgs.lib.hasInfix "orchestratorRuntimeShell = import ./orchestrator-runtime.nix"
  orchestratorSource;
assert pkgs.lib.hasInfix "\${orchestratorRuntimeShell}" orchestratorSource;
assert (!pkgs.lib.hasInfix "split_process_mode() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "validate_workflow_args() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "validate_typed_task_args() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "resolve_run_artifacts_dir() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -r '.state'" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -r '.pid // empty'" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -r '.pgid // empty'" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -cS '.' \"$run_file\"" orchestratorSource);
assert pkgs.lib.hasInfix "run_file_state \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "run_file_pid \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "run_file_pgid \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "run_file_attempt_id \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "run_file_command \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "run_file_process_mode \"$run_file\"" orchestratorSource;
assert pkgs.lib.hasInfix "registry_events_index_snapshot" orchestratorSource;
assert pkgs.lib.hasInfix "write_run_record_json_file \"$tmp\"" orchestratorSource;
assert pkgs.lib.hasInfix "cat \"$run_file\"" orchestratorSource;
assert (!pkgs.lib.hasInfix "jq -r --arg runId" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -cS" orchestratorSource);
assert pkgs.lib.hasInfix "registry_events_index_snapshot" controlSource;
assert (!pkgs.lib.hasInfix "jq -r --arg runId" controlSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -cS" controlSource);
assert pkgs.lib.hasInfix "commonRuntimeShell = import ./common-runtime.nix" runtimeSource;
assert pkgs.lib.hasInfix "\${commonRuntimeShell}" runtimeSource;
assert pkgs.lib.hasInfix "split_process_mode() {" runtimeSource;
assert pkgs.lib.hasInfix "run_record_fields_file() {" runtimeSource;
assert pkgs.lib.hasInfix "write_run_record_fields() {" runtimeSource;
assert pkgs.lib.hasInfix "run_record_history_append() {" runtimeSource;
assert pkgs.lib.hasInfix "write_run_record_json_file() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_state() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_pid() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_pgid() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_attempt_id() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_command() {" runtimeSource;
assert pkgs.lib.hasInfix "run_file_process_mode() {" runtimeSource;
assert (!pkgs.lib.hasInfix "write_text_file_atomic() {" runtimeSource);
assert pkgs.lib.hasInfix "validate_workflow_args() {" runtimeSource;
assert pkgs.lib.hasInfix "validate_typed_task_args() {" runtimeSource;
assert pkgs.lib.hasInfix "resolve_run_artifacts_dir() {" runtimeSource;
assert pkgs.lib.hasInfix "ensure_artifacts_root() {" runtimeSource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" runtimeSource);
assert pkgs.lib.hasInfix "json_object_from_named_env_values() {" commonSource;
assert pkgs.lib.hasInfix "NIXFIED_RUN_ID_FILE_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "NIXFIED_SUMMARY_FILE_OVERRIDE" runtimeSource;
assert pkgs.lib.hasInfix "workflow_simple_shorthand_exists_for_family" runtimeSource;
assert pkgs.lib.hasInfix "task_arg_long_kind" runtimeSource;
assert (!pkgs.lib.hasInfix "NIXFIED_JSON_OUTPUT_OVERRIDE" runtimeSource);
assert (!pkgs.lib.hasInfix "--json" runtimeSource);
assert pkgs.lib.hasInfix "NIXFIED_ORCHESTRATOR_STOP_TIMEOUT_SECONDS" orchestratorSource;
assert pkgs.lib.hasInfix "handle_foreground_signal() {" orchestratorSource;
assert pkgs.lib.hasInfix "valid_log_level() {" commonSource;
assert pkgs.lib.hasInfix "valid_output_mode() {" commonSource;
assert pkgs.lib.hasInfix "write_text_file_atomic() {" commonSource;
assert pkgs.lib.hasInfix "ERROR: unable to create temp file for '$target'" commonSource;
pkgs.runCommand "orchestrator-runtime-contract" { } ''
  echo "OK: orchestrator runtime helpers are split and stable" > "$out"
''
