{ pkgs }:
let
  orchestratorSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator.nix;
  dispatcherSource = builtins.readFile ../../nixfied/framework/runtime/dispatcher.nix;
  executorSource = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  sharedRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/shared-runtime-lib.nix;
  hasSharedRuntimeInfix =
    pattern:
    pkgs.lib.hasInfix pattern orchestratorSource || pkgs.lib.hasInfix pattern sharedRuntimeSource;
in
assert pkgs.lib.hasInfix "nixfied-orchestrator" orchestratorSource;
assert pkgs.lib.hasInfix "create_run_record() {" orchestratorSource;
assert pkgs.lib.hasInfix "local attempt_id=\"$2\"" orchestratorSource;
assert pkgs.lib.hasInfix "update_run_state() {" orchestratorSource;
assert pkgs.lib.hasInfix "registry_events_index_snapshot" orchestratorSource;
assert pkgs.lib.hasInfix "\${pkgs.procps}/bin/ps" orchestratorSource;
assert pkgs.lib.hasInfix "nixfied-kernel run-record create" orchestratorSource;
assert pkgs.lib.hasInfix "nixfied-kernel run-record transition" orchestratorSource;
assert pkgs.lib.hasInfix "stop-all-runs" orchestratorSource;
assert pkgs.lib.hasInfix "orchestratorRuntimeShell = import ./orchestrator-runtime.nix"
  orchestratorSource;
assert pkgs.lib.hasInfix "\${orchestratorRuntimeShell}" orchestratorSource;
assert pkgs.lib.hasInfix "split_process_mode \"$@\"" orchestratorSource;
assert pkgs.lib.hasInfix "validate_typed_task_args \"$task_id\"" orchestratorSource;
assert pkgs.lib.hasInfix "ensure_artifacts_root \"$run_id\" \"$ephemeral_enabled\""
  orchestratorSource;
assert pkgs.lib.hasInfix "workflow_id_exists \"$workflow_id\"" orchestratorSource;
assert (!pkgs.lib.hasInfix "NIXFIED_MODEL_FILE" orchestratorSource);
assert (!pkgs.lib.hasInfix "task_json() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "workflow_json() {" orchestratorSource);
assert (!pkgs.lib.hasInfix "jq -r --arg runId" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -cnS" orchestratorSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq -cS" orchestratorSource);
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" executorSource;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" executorSource;
assert pkgs.lib.hasInfix "NIXFIED_ORCHESTRATOR_RUN_ID" executorSource;
assert pkgs.lib.hasInfix "NIXFIED_ORCHESTRATOR_ATTEMPT_ID" executorSource;
assert pkgs.lib.hasInfix "frameworkEphemeral = import ./ephemeral.nix" orchestratorSource;
assert pkgs.lib.hasInfix "ephemeral = model.runtime.ephemeral or { };" orchestratorSource;
assert pkgs.lib.hasInfix "EPHEMERAL_EXECUTOR_WRAPPER=" orchestratorSource;
assert hasSharedRuntimeInfix "kernel_run_id_envelope() {";
assert hasSharedRuntimeInfix "nixfied-kernel run-id envelope";
assert pkgs.lib.hasInfix "kernel_event_detail() {" orchestratorSource;
assert pkgs.lib.hasInfix "nixfied-kernel event-detail render" orchestratorSource;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_SETUP_STARTED_AT" orchestratorSource;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_SETUP_STARTED_EPOCH" orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-task"
  orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-workflow"
  orchestratorSource;
assert pkgs.lib.hasInfix "orchestratorProgram =" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-task" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-workflow" dispatcherSource;
assert pkgs.lib.hasInfix "framework_source_flake_ref=" dispatcherSource;
assert pkgs.lib.hasInfix "NIXFIED_FRAMEWORK_SOURCE_FLAKE" dispatcherSource;
assert pkgs.lib.hasInfix "frameworkSourceFlakeRefShell" dispatcherSource;
assert pkgs.lib.hasInfix "#run-task" dispatcherSource;
assert pkgs.lib.hasInfix "task.framework.install" dispatcherSource;
assert pkgs.lib.hasInfix "task.framework.upgrade" dispatcherSource;
assert pkgs.lib.hasInfix "--refresh --" dispatcherSource;
assert pkgs.lib.hasInfix "github:willyrgf/nixfied/dev" dispatcherSource;
assert pkgs.lib.hasInfix "frameworkUpgradeHelpFile" dispatcherSource;
assert pkgs.lib.hasInfix "proxyFrameworkCommand" dispatcherSource;
assert (!pkgs.lib.hasInfix "json_quote_string" orchestratorSource);
assert (!pkgs.lib.hasInfix "positional_args_json" orchestratorSource);
pkgs.runCommand "orchestrator-lifecycle-contract" { } ''
  echo "OK: orchestrator lifecycle contracts are stable" > "$out"
''
