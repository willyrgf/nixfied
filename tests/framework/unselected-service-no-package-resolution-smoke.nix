{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./launcher-helios-task-module.nix ];
    selectedServices = [ ];
    localOverrides = [
      (
        { lib, ... }:
        {
          nixfied.services.helios = {
            enable = lib.mkForce true;
            sourceKeys = lib.mkForce [ "poison" ];
            defaultSource = lib.mkForce "poison";
            sources.poison.packageFactory = ./poison-package.nix;
          };
        }
      )
    ];
  };
in
pkgs.runCommand "unselected-service-no-package-resolution-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${compiled.apps.run-task.program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "$RUN_TASK_APP" task.test.launcher.control > "$TMPDIR/control.out" 2>&1 || {
    cat "$TMPDIR/control.out"
    fail "control task should not resolve packages for unselected services"
  }

  require_contains "$TMPDIR/control.out" "OK: launcher control task ran"
  require_not_contains "$TMPDIR/control.out" "service package resolved unexpectedly for an unselected service"

  echo "OK: unselected services do not resolve runtime packages" > "$out"
''
