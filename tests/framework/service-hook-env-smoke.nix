{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  noServiceHookTaskId = "task.test.service-hooks.none";
  postgresHookTaskId = "task.test.service-hooks.postgres";
  depClosureHookTaskId = "task.test.service-hooks.dep-closure";

  envKeysFor = services:
    builtins.concatStringsSep "\n" (
      builtins.sort builtins.lessThan (
        [ "NIXFIED_SERVICE_ROOT" ]
        ++ builtins.concatMap (
        service:
        map (suffix: "NIXFIED_SERVICE_${lib.toUpper service}_${suffix}") [
          "DATA_DIR"
          "LOG_DIR"
          "STATE_DIR"
        ]
      ) services
      )
    );

  serviceEnvContractPrelude = expectedKeys: ''
    actual_keys="$(
      env \
        | ${pkgs.gnugrep}/bin/grep '^NIXFIED_SERVICE_' \
        | ${pkgs.coreutils}/bin/cut -d= -f1 \
        | ${pkgs.coreutils}/bin/sort
    )"
    expected_keys=${lib.escapeShellArg expectedKeys}
    if [ "$actual_keys" != "$expected_keys" ]; then
      echo "ERROR: unexpected NIXFIED_SERVICE_* key set"
      printf 'expected:\n%s\nactual:\n%s\n' "$expected_keys" "$actual_keys"
      exit 1
    fi
  '';

  mkHookTask =
    {
      taskId,
      summary,
      description,
      requiredServices ? [ ],
      depsNeeds ? [ ],
      command,
    }:
    {
      inherit summary description;
      id = taskId;
      requirements.services = requiredServices;
      deps.needs = depsNeeds;
      commandApi.commandClass = "passthrough";
      runner.command = command;
    };

  serviceHookModule =
    { lib, ... }:
    {
      nixfied.services.postgres.enable = lib.mkForce true;
      nixfied.services.nginx.enable = lib.mkForce true;

      nixfied.tasks."test.service-hooks.none" = mkHookTask {
        taskId = noServiceHookTaskId;
        summary = "Service hook export break contract";
        description = "Tasks without selected services should not receive ambient service hooks.";
        command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude (envKeysFor [ ])}

          test -z "''${SVC_POSTGRES_STATUS:-}"
          test -z "''${SVC_POSTGRES_PREFLIGHT_START:-}"
          test -z "''${SVC_NGINX_STATUS:-}"
          test -z "''${SVC_NGINX_PREFLIGHT_START:-}"

          echo "OK: ambient service hooks removed"
        '';
      };

      nixfied.tasks."test.service-hooks.postgres" = mkHookTask {
        taskId = postgresHookTaskId;
        summary = "Scoped postgres hook export smoke";
        description = "Tasks with postgres selected should receive postgres hooks only.";
        requiredServices = [ "postgres" ];
        command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude (envKeysFor [ "postgres" ])}

          test -n "''${SVC_POSTGRES_STATUS:-}"
          test -x "$SVC_POSTGRES_STATUS"
          test -z "''${SVC_POSTGRES_PREFLIGHT_START:-}"
          test -z "''${SVC_NGINX_STATUS:-}"

          echo "OK: scoped postgres hook present"
        '';
      };

      nixfied.tasks."test.service-hooks.dep-closure" = mkHookTask {
        taskId = depClosureHookTaskId;
        summary = "Dependency-closure hook export smoke";
        description = "Tasks inherit selected service hooks from required task dependencies.";
        depsNeeds = [ postgresHookTaskId ];
        command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude (envKeysFor [ "postgres" ])}

          test -n "''${SVC_POSTGRES_STATUS:-}"
          test -x "$SVC_POSTGRES_STATUS"
          test -z "''${SVC_POSTGRES_PREFLIGHT_START:-}"
          test -z "''${SVC_NGINX_STATUS:-}"

          echo "OK: dependency closure hook present"
        '';
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
assert builtins.hasAttr noServiceHookTaskId compiledIncluded.model.tasks;
assert builtins.hasAttr postgresHookTaskId compiledIncluded.model.tasks;
assert builtins.hasAttr depClosureHookTaskId compiledIncluded.model.tasks;
assert builtins.hasAttr "svc::postgres::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::nginx::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::postgres::status" compiledExcluded.apps;
assert !(builtins.hasAttr "svc::nginx::status" compiledExcluded.apps);
assert !(builtins.any (
  key: lib.hasPrefix "SVC_NGINX_" key
) (builtins.attrNames (compiledExcluded.model.runtime.hookEnv or { })));
pkgs.runCommand "service-hook-env-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  run_task_expect_success() {
    local label="$1"
    local out_file="$2"
    shift 2

    set +e
    "$@" > "$out_file" 2>&1
    rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      cat "$out_file"
      fail "run-task failed label=$label rc=$rc"
    fi
  }

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  INCLUDED_RUN_TASK="${compiledIncluded.apps.run-task.program}"
  EXCLUDED_RUN_TASK="${compiledExcluded.apps.run-task.program}"

  run_task_expect_success included-no-hooks "$TMPDIR/no-hooks.out" "$INCLUDED_RUN_TASK" ${noServiceHookTaskId}
  require_contains "$TMPDIR/no-hooks.out" "OK: ambient service hooks removed"

  run_task_expect_success included-postgres "$TMPDIR/postgres-only.out" "$INCLUDED_RUN_TASK" ${postgresHookTaskId}
  require_contains "$TMPDIR/postgres-only.out" "OK: scoped postgres hook present"

  run_task_expect_success included-dep-closure "$TMPDIR/dep-closure.out" "$INCLUDED_RUN_TASK" ${depClosureHookTaskId}
  require_contains "$TMPDIR/dep-closure.out" "OK: dependency closure hook present"

  run_task_expect_success excluded-no-hooks "$TMPDIR/excluded-no-hooks.out" "$EXCLUDED_RUN_TASK" ${noServiceHookTaskId}
  require_contains "$TMPDIR/excluded-no-hooks.out" "OK: ambient service hooks removed"

  run_task_expect_success excluded-postgres "$TMPDIR/excluded-postgres.out" "$EXCLUDED_RUN_TASK" ${postgresHookTaskId}
  require_contains "$TMPDIR/excluded-postgres.out" "OK: scoped postgres hook present"

  run_task_expect_success excluded-dep-closure "$TMPDIR/excluded-dep-closure.out" "$EXCLUDED_RUN_TASK" ${depClosureHookTaskId}
  require_contains "$TMPDIR/excluded-dep-closure.out" "OK: dependency closure hook present"

  echo "OK: service hook env export is scoped by explicit service selection" > "$out"
''
