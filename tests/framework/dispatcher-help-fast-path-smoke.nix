{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ./launcher-disabled-nginx-override.nix ];
  };
in
pkgs.runCommand "dispatcher-help-fast-path-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"
  RUN_WORKFLOW_APP="${frameworkOutputs.apps."run-workflow".program}"
  RUN_WORKFLOW_PARALLEL_APP="${frameworkOutputs.apps."run-workflow-parallel".program}"
  RUNS_APP="${frameworkOutputs.apps.runs.program}"
  STOP_RUN_APP="${frameworkOutputs.apps."stop-run".program}"
  STOP_ALL_RUNS_APP="${frameworkOutputs.apps."stop-all-runs".program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "$RUN_TASK_APP" --help > "$TMPDIR/run-task-help.out" 2>&1
  require_contains "$TMPDIR/run-task-help.out" "run-task - Run a compiled task by id"
  require_contains "$TMPDIR/run-task-help.out" "nix run .#run-task -- <task-id> [-- ...]"
  require_not_contains "$TMPDIR/run-task-help.out" "nginx evaluated unexpectedly"

  "$RUN_WORKFLOW_APP" --help > "$TMPDIR/run-workflow-help.out" 2>&1
  require_contains "$TMPDIR/run-workflow-help.out" "run-workflow - Run a compiled workflow by id"
  require_contains "$TMPDIR/run-workflow-help.out" "nix run .#run-workflow -- <workflow-id> [-- ...]"
  require_not_contains "$TMPDIR/run-workflow-help.out" "nginx evaluated unexpectedly"

  "$RUN_WORKFLOW_PARALLEL_APP" --help > "$TMPDIR/run-workflow-parallel-help.out" 2>&1
  require_contains "$TMPDIR/run-workflow-parallel-help.out" "run-workflow-parallel - Run a compiled workflow by id with parallel execution enabled"
  require_contains "$TMPDIR/run-workflow-parallel-help.out" "nix run .#run-workflow-parallel -- <workflow-id> [-- ...]"
  require_not_contains "$TMPDIR/run-workflow-parallel-help.out" "nginx evaluated unexpectedly"

  "$RUNS_APP" --help > "$TMPDIR/runs-help.out" 2>&1
  require_contains "$TMPDIR/runs-help.out" "runs - List runs or show one run by id"
  require_contains "$TMPDIR/runs-help.out" "nix run .#runs"
  require_not_contains "$TMPDIR/runs-help.out" "nginx evaluated unexpectedly"

  "$STOP_RUN_APP" --help > "$TMPDIR/stop-run-help.out" 2>&1
  require_contains "$TMPDIR/stop-run-help.out" "stop-run - Stop one running or queued run"
  require_contains "$TMPDIR/stop-run-help.out" "nix run .#stop-run -- <run-id>"
  require_not_contains "$TMPDIR/stop-run-help.out" "nginx evaluated unexpectedly"

  "$STOP_ALL_RUNS_APP" --help > "$TMPDIR/stop-all-runs-help.out" 2>&1
  require_contains "$TMPDIR/stop-all-runs-help.out" "stop-all-runs - Stop all running or queued runs"
  require_contains "$TMPDIR/stop-all-runs-help.out" "nix run .#stop-all-runs"
  require_not_contains "$TMPDIR/stop-all-runs-help.out" "nginx evaluated unexpectedly"

  echo "OK: dispatcher and control help paths avoid service materialization" > "$out"
''
