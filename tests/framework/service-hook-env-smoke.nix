{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  serviceHookTaskId = "task.test.service-hooks";

  serviceHookModule = {
    nixfied.tasks."test.service-hooks" = {
      id = serviceHookTaskId;
      summary = "Service hook export smoke";
      description = "Validates service hook env export for included and excluded services.";
      contract.input.args.parser = "passthrough";
      contract.input.args.allowUnknown = true;
      runner.command = ''
        set -euo pipefail

        mode="''${1:-}"
        case "$mode" in
          included|excluded) ;;
          *)
            echo "ERROR: usage: task.test.service-hooks <included|excluded>" >&2
            exit 2
            ;;
        esac

        if [ -z "''${SVC_POSTGRES_STATUS:-}" ]; then
          echo "ERROR: missing postgres service hook"
          exit 1
        fi
        if [ ! -x "$SVC_POSTGRES_STATUS" ]; then
          echo "ERROR: postgres service hook is not executable path=$SVC_POSTGRES_STATUS"
          exit 1
        fi

        case "$mode" in
          included)
            if [ -z "''${SVC_NGINX_STATUS:-}" ]; then
              echo "ERROR: missing nginx service hook in included mode"
              exit 1
            fi
            if [ ! -x "$SVC_NGINX_STATUS" ]; then
              echo "ERROR: nginx service hook is not executable path=$SVC_NGINX_STATUS"
              exit 1
            fi
            ;;
          excluded)
            if [ -n "''${SVC_NGINX_STATUS:-}" ]; then
              echo "ERROR: nginx service hook should be absent when nginx is excluded"
              exit 1
            fi
            ;;
        esac

        echo "OK: service hooks mode=$mode postgres=present nginx=''${SVC_NGINX_STATUS:+present}"
      '';
      ui.app = {
        expose = false;
        name = "test-service-hooks";
      };
    };
  };

  compiledIncluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ serviceHookModule ];
    localOverrides = [ ];
  };

  compiledExcluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ serviceHookModule ];
    localOverrides = [
      (
        { ... }:
        {
          nixfied.graph.excludedServices = [ "nginx" ];
        }
      )
    ];
  };
in
assert builtins.hasAttr "svc::postgres::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::nginx::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::postgres::status" compiledExcluded.apps;
assert !(builtins.hasAttr "svc::nginx::status" compiledExcluded.apps);
pkgs.runCommand "service-hook-env-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  INCLUDED_RUN_TASK="${compiledIncluded.apps.run-task.program}"
  EXCLUDED_RUN_TASK="${compiledExcluded.apps.run-task.program}"

  "$INCLUDED_RUN_TASK" ${serviceHookTaskId} included > "$TMPDIR/included.out" 2>&1
  require_contains "$TMPDIR/included.out" "OK: service hooks mode=included postgres=present nginx=present"

  "$EXCLUDED_RUN_TASK" ${serviceHookTaskId} excluded > "$TMPDIR/excluded.out" 2>&1
  require_contains "$TMPDIR/excluded.out" "OK: service hooks mode=excluded postgres=present nginx="

  echo "OK: service hook env export tracks the compiled service graph" > "$out"
''
