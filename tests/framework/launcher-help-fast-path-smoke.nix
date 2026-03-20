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
pkgs.runCommand "launcher-help-fast-path-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"
  CI_APP="${frameworkOutputs.apps.ci.program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "$RUN_TASK_APP" task.test.isolation.unit --help > "$TMPDIR/run-task-help.out" 2>&1
  require_contains "$TMPDIR/run-task-help.out" "task.test.isolation.unit - Isolation probe unit"
  require_contains "$TMPDIR/run-task-help.out" "Usage:"
  require_not_contains "$TMPDIR/run-task-help.out" "nginx evaluated unexpectedly"

  "$CI_APP" --help > "$TMPDIR/ci-help.out" 2>&1
  require_contains "$TMPDIR/ci-help.out" "ci - Run the CI pipeline"
  require_contains "$TMPDIR/ci-help.out" "Usage:"
  require_not_contains "$TMPDIR/ci-help.out" "nginx evaluated unexpectedly"

  echo "OK: launcher metadata help paths avoid disabled-service leakage" > "$out"
''
