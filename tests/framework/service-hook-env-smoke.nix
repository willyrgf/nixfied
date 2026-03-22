{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  noServiceHookTaskId = "task.test.service-hooks.none";
  postgresHookTaskId = "task.test.service-hooks.postgres";
  bothHooksTaskId = "task.test.service-hooks.both";
  depClosureHookTaskId = "task.test.service-hooks.dep-closure";

  noServiceEnvKeys = builtins.concatStringsSep "\n" [
    "NIXFIED_SERVICE_ROOT"
  ];
  postgresEnvKeys = builtins.concatStringsSep "\n" [
    "NIXFIED_SERVICE_POSTGRES_DATA_DIR"
    "NIXFIED_SERVICE_POSTGRES_LOG_DIR"
    "NIXFIED_SERVICE_POSTGRES_STATE_DIR"
    "NIXFIED_SERVICE_ROOT"
  ];
  bothEnvKeys = builtins.concatStringsSep "\n" [
    "NIXFIED_SERVICE_NGINX_DATA_DIR"
    "NIXFIED_SERVICE_NGINX_LOG_DIR"
    "NIXFIED_SERVICE_NGINX_STATE_DIR"
    "NIXFIED_SERVICE_POSTGRES_DATA_DIR"
    "NIXFIED_SERVICE_POSTGRES_LOG_DIR"
    "NIXFIED_SERVICE_POSTGRES_STATE_DIR"
    "NIXFIED_SERVICE_ROOT"
  ];

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

  serviceHookModule =
    { lib, ... }:
    {
      nixfied.services.postgres.enable = lib.mkForce true;
      nixfied.services.nginx.enable = lib.mkForce true;

      nixfied.tasks."test.service-hooks.none" = {
        id = noServiceHookTaskId;
        summary = "Service hook export break contract";
        description = "Tasks without selected services should not receive ambient service hooks.";
        contract.input.args.parser = "passthrough";
        contract.input.args.allowUnknown = true;
        runner.command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude noServiceEnvKeys}

          if [ -n "''${SVC_POSTGRES_STATUS:-}" ]; then
            echo "ERROR: postgres service hook should be absent without explicit selection"
            exit 1
          fi
          if [ -n "''${SVC_NGINX_STATUS:-}" ]; then
            echo "ERROR: nginx service hook should be absent without explicit selection"
            exit 1
          fi

          echo "OK: ambient service hooks removed"
        '';
      };

      nixfied.tasks."test.service-hooks.postgres" = {
        id = postgresHookTaskId;
        summary = "Scoped postgres hook export smoke";
        description = "Tasks with postgres selected should receive postgres hooks only.";
        requirements.services = [ "postgres" ];
        contract.input.args.parser = "passthrough";
        contract.input.args.allowUnknown = true;
        runner.command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude postgresEnvKeys}

          if [ -z "''${SVC_POSTGRES_STATUS:-}" ]; then
            echo "ERROR: missing postgres service hook"
            exit 1
          fi
          if [ ! -x "$SVC_POSTGRES_STATUS" ]; then
            echo "ERROR: postgres service hook is not executable path=$SVC_POSTGRES_STATUS"
            exit 1
          fi
          if [ -n "''${SVC_NGINX_STATUS:-}" ]; then
            echo "ERROR: nginx service hook should be absent when not selected"
            exit 1
          fi

          echo "OK: scoped postgres hook present"
        '';
      };

      nixfied.tasks."test.service-hooks.both" = {
        id = bothHooksTaskId;
        summary = "Scoped postgres+nginx hook export smoke";
        description = "Tasks with both services selected should receive both hooks.";
        requirements.services = [
          "postgres"
          "nginx"
        ];
        contract.input.args.parser = "passthrough";
        contract.input.args.allowUnknown = true;
        runner.command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude bothEnvKeys}

          if [ -z "''${SVC_POSTGRES_STATUS:-}" ]; then
            echo "ERROR: missing postgres service hook"
            exit 1
          fi
          if [ ! -x "$SVC_POSTGRES_STATUS" ]; then
            echo "ERROR: postgres service hook is not executable path=$SVC_POSTGRES_STATUS"
            exit 1
          fi
          if [ -z "''${SVC_NGINX_STATUS:-}" ]; then
            echo "ERROR: missing nginx service hook"
            exit 1
          fi
          if [ ! -x "$SVC_NGINX_STATUS" ]; then
            echo "ERROR: nginx service hook is not executable path=$SVC_NGINX_STATUS"
            exit 1
          fi

          echo "OK: scoped postgres and nginx hooks present"
        '';
      };

      nixfied.tasks."test.service-hooks.dep-closure" = {
        id = depClosureHookTaskId;
        summary = "Dependency-closure hook export smoke";
        description = "Tasks inherit selected service hooks from required task dependencies.";
        deps.needs = [ postgresHookTaskId ];
        contract.input.args.parser = "passthrough";
        contract.input.args.allowUnknown = true;
        runner.command = ''
          set -euo pipefail
          ${serviceEnvContractPrelude postgresEnvKeys}

          if [ -z "''${SVC_POSTGRES_STATUS:-}" ]; then
            echo "ERROR: missing postgres service hook from dependency closure"
            exit 1
          fi
          if [ ! -x "$SVC_POSTGRES_STATUS" ]; then
            echo "ERROR: postgres service hook is not executable path=$SVC_POSTGRES_STATUS"
            exit 1
          fi
          if [ -n "''${SVC_NGINX_STATUS:-}" ]; then
            echo "ERROR: nginx service hook should be absent when only dependency-selected postgres is required"
            exit 1
          fi

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
assert builtins.hasAttr bothHooksTaskId compiledIncluded.model.tasks;
assert builtins.hasAttr depClosureHookTaskId compiledIncluded.model.tasks;
assert builtins.hasAttr "svc::postgres::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::nginx::status" compiledIncluded.apps;
assert builtins.hasAttr "svc::postgres::status" compiledExcluded.apps;
assert !(builtins.hasAttr "svc::nginx::status" compiledExcluded.apps);
assert builtins.hasAttr noServiceHookTaskId compiledExcluded.model.tasks;
assert builtins.hasAttr postgresHookTaskId compiledExcluded.model.tasks;
assert builtins.hasAttr depClosureHookTaskId compiledExcluded.model.tasks;
assert !(builtins.hasAttr bothHooksTaskId compiledExcluded.model.tasks);
pkgs.runCommand "service-hook-env-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  INCLUDED_RUN_TASK="${compiledIncluded.apps.run-task.program}"
  EXCLUDED_RUN_TASK="${compiledExcluded.apps.run-task.program}"

  "$INCLUDED_RUN_TASK" ${noServiceHookTaskId} > "$TMPDIR/no-hooks.out" 2>&1
  require_contains "$TMPDIR/no-hooks.out" "OK: ambient service hooks removed"

  "$INCLUDED_RUN_TASK" ${postgresHookTaskId} > "$TMPDIR/postgres-only.out" 2>&1
  require_contains "$TMPDIR/postgres-only.out" "OK: scoped postgres hook present"

  "$INCLUDED_RUN_TASK" ${bothHooksTaskId} > "$TMPDIR/both-hooks.out" 2>&1
  require_contains "$TMPDIR/both-hooks.out" "OK: scoped postgres and nginx hooks present"

  "$INCLUDED_RUN_TASK" ${depClosureHookTaskId} > "$TMPDIR/dep-closure.out" 2>&1
  require_contains "$TMPDIR/dep-closure.out" "OK: dependency closure hook present"

  "$EXCLUDED_RUN_TASK" ${noServiceHookTaskId} > "$TMPDIR/excluded-no-hooks.out" 2>&1
  require_contains "$TMPDIR/excluded-no-hooks.out" "OK: ambient service hooks removed"

  "$EXCLUDED_RUN_TASK" ${postgresHookTaskId} > "$TMPDIR/excluded-postgres.out" 2>&1
  require_contains "$TMPDIR/excluded-postgres.out" "OK: scoped postgres hook present"

  "$EXCLUDED_RUN_TASK" ${depClosureHookTaskId} > "$TMPDIR/excluded-dep-closure.out" 2>&1
  require_contains "$TMPDIR/excluded-dep-closure.out" "OK: dependency closure hook present"

  echo "OK: service hook env export is scoped by explicit service selection" > "$out"
''
