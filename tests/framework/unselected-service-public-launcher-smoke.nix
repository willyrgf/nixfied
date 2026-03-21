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
    extraModules = [ ./launcher-helios-task-module.nix ];
    localOverrides = [ ./poison-helios-source-override.nix ];
  };
in
pkgs.runCommand "unselected-service-public-launcher-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "$RUN_TASK_APP" task.test.launcher.control > "$TMPDIR/control.out" 2>&1 || {
    cat "$TMPDIR/control.out"
    fail "public run-task launcher should not resolve packages for unselected services"
  }

  require_contains "$TMPDIR/control.out" "OK: launcher control task ran"
  require_not_contains "$TMPDIR/control.out" "service package resolved unexpectedly for an unselected service"

  echo "OK: public run-task launcher skips package resolution for unselected services" > "$out"
''
