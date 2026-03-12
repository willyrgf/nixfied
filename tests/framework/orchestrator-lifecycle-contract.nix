{ pkgs }:
let
  orchestratorSource = builtins.readFile ../../nixfied/runner/orchestrator.nix;
  dispatcherSource = builtins.readFile ../../nixfied/runner/dispatcher.nix;
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
in
assert pkgs.lib.hasInfix "nixfied-orchestrator" orchestratorSource;
assert pkgs.lib.hasInfix "create_run_record() {" orchestratorSource;
assert pkgs.lib.hasInfix "update_run_state() {" orchestratorSource;
assert pkgs.lib.hasInfix "registry_events_snapshot" orchestratorSource;
assert pkgs.lib.hasInfix "\${pkgs.procps}/bin/ps" orchestratorSource;
assert pkgs.lib.hasInfix "tmp=\"$(mktemp \"$run_file.tmp.XXXXXX\")\"" orchestratorSource;
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
assert pkgs.lib.hasInfix "run_workflow_phase_tasks() {" executorSource;
assert pkgs.lib.hasInfix "write_workflow_summary_json() {" executorSource;
assert pkgs.lib.hasInfix "NIXFIED_ORCHESTRATOR_RUN_ID" executorSource;
assert pkgs.lib.hasInfix "frameworkEphemeral = import ../framework/runtime/ephemeral.nix"
  orchestratorSource;
assert pkgs.lib.hasInfix "ephemeral = model.runtime.ephemeral or { };" orchestratorSource;
assert pkgs.lib.hasInfix "EPHEMERAL_EXECUTOR_WRAPPER=" orchestratorSource;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_SETUP_STARTED_AT" orchestratorSource;
assert pkgs.lib.hasInfix "NIXFIED_WORKFLOW_SETUP_STARTED_EPOCH" orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-task"
  orchestratorSource;
assert pkgs.lib.hasInfix "\"$EPHEMERAL_EXECUTOR_WRAPPER\" \"$EXECUTOR_PROGRAM\" run-workflow"
  orchestratorSource;
assert pkgs.lib.hasInfix "orchestratorProgram =" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-task" dispatcherSource;
assert pkgs.lib.hasInfix "exec \${orchestratorProgram} run-workflow" dispatcherSource;
assert pkgs.lib.hasInfix "github:willyrgf/nixfied/dev#framework::install --refresh --"
  dispatcherSource;
assert pkgs.lib.hasInfix "github:willyrgf/nixfied/dev#framework::upgrade --refresh --"
  dispatcherSource;
assert pkgs.lib.hasInfix "cat \${frameworkUpgradeHelpFile}" dispatcherSource;
pkgs.runCommand "orchestrator-lifecycle-contract" { } ''
  echo "OK: orchestrator lifecycle contracts are stable" > "$out"
''
