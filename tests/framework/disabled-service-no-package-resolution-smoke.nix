{ pkgs }:
let
  inherit (pkgs) lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  controlTaskId = "task.test.disabled-service.control";

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.tasks."test.disabled-service.control" = {
          id = controlTaskId;
          summary = "Disabled service control task";
          description = "Runs without touching disabled service packages.";
          runner.command = ''
            set -euo pipefail
            printf '%s\n' "OK: disabled-service control task ran"
          '';
        };
      }
    ];
    localOverrides = [
      (
        { lib, ... }:
        {
          nixfied.services.helios = {
            enable = lib.mkForce false;
            sourceKeys = lib.mkForce [ "poison" ];
            defaultSource = lib.mkForce "poison";
            sources.poison.packageFactory = ./poison-package.nix;
          };
        }
      )
    ];
  };

  runTaskProgram = frameworkOutputs.apps.run-task.program;
  appNames = builtins.sort builtins.lessThan (builtins.attrNames frameworkOutputs.apps);
in
assert builtins.isString runTaskProgram;
assert !(builtins.any (appName: lib.hasPrefix "svc::helios::" appName) appNames);
pkgs.runCommand "disabled-service-no-package-resolution-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "${runTaskProgram}" "${controlTaskId}" > "$TMPDIR/control.out" 2>&1 || {
    cat "$TMPDIR/control.out"
    fail "public run-task launcher should not resolve packages for disabled services"
  }

  require_contains "$TMPDIR/control.out" "OK: disabled-service control task ran"
  require_not_contains "$TMPDIR/control.out" "service package resolved unexpectedly for an unselected service"

  echo "OK: disabled services do not resolve poisoned packages or publish svc app surfaces" > "$out"
''
