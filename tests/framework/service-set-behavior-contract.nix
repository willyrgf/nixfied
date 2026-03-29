{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = ../..;
  workflowId = "workflow.test.service-set.phase";
  taskId = "task.test.service-set.phase.body";

  workflowModule = {
    nixfied.tasks."test.service-set.phase.body" = {
      id = taskId;
      summary = "service-set phase body";
      description = "Emits a marker so workflow phase ordering stays testable.";
      runner.command = ''
        set -euo pipefail
        printf '%s\n' "BODY"
      '';
    };

    nixfied.workflows."test.service-set.phase" = {
      id = workflowId;
      summary = "Workflow phase service-set behavior contract";
      description = "Exercises grouped service-set export and workflow phase adapters.";
      units.main.taskId = taskId;
      preRun.serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "status";
        }
      ];
      postRun = {
        serviceSets = [
          {
            serviceSetId = "service-set.default";
            operation = "status";
          }
        ];
        alwaysRun = true;
      };
    };
  };

  exportOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      ./service-set-enabled-module.nix
    ];
    localOverrides = [ ];
  };

  workflowOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ workflowModule ];
    localOverrides = [ ];
  };

  disabledOutputs = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ./launcher-disabled-nginx-override.nix ];
  };

  disabledHookNames = builtins.attrNames disabledOutputs.serviceHookEnv;
  disabledAppNames = builtins.attrNames disabledOutputs.apps;
in
assert !(builtins.any (hookName: lib.hasPrefix "SVC_NGINX_" hookName) disabledHookNames);
assert !(builtins.any (appName: lib.hasPrefix "svc::nginx::" appName) disabledAppNames);
pkgs.runCommand "service-set-behavior-contract" { src = repoRoot; } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  workspace_tmp="$(${pkgs.coreutils}/bin/mktemp -d "''${TMPDIR:-/tmp}/nixfied-service-set-behavior.XXXXXX")"
  trap 'rm -rf "$workspace_tmp"' EXIT
  export TMPDIR="$workspace_tmp"
  EXPORT_APP="${exportOutputs.apps."services-export".program}"
  INTROSPECT_APP="${exportOutputs.apps.introspect.program}"
  RUN_WORKFLOW_APP="${workflowOutputs.apps."run-workflow".program}"
  JQ=${pkgs.jq}/bin/jq
  export NIXFIED_FLAKE_ROOT="$src"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  cd "$NIXFIED_FLAKE_ROOT"

  "$EXPORT_APP" --format json > "$TMPDIR/service-set.json"
  "$JQ" -e '.kind == "service-set-export" and .version == 1' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.serviceSetId == "service-set.default"' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.services | map(.service) | sort == ["minio", "postgres"]' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.services | map(.resolvedArtifacts | has("dataDir")) | all' "$TMPDIR/service-set.json" > /dev/null

  "$INTROSPECT_APP" service-set:default --json > "$TMPDIR/service-set-introspect.json"
  "$JQ" -e '.payload.resolved.nodeId == "service-set:default"' "$TMPDIR/service-set-introspect.json" > /dev/null
  "$JQ" -e '.payload.resolution.data.requiredServices | sort == ["minio", "postgres"]' \
    "$TMPDIR/service-set-introspect.json" > /dev/null

  "$RUN_WORKFLOW_APP" ${lib.escapeShellArg workflowId} > "$TMPDIR/workflow.out" 2>&1 || {
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set contract should succeed"
  }

  require_contains "$TMPDIR/workflow.out" "BODY"
  require_contains "$TMPDIR/workflow.out" "OK: service-set default status passed services=0"

  status_count="$(${pkgs.gnugrep}/bin/grep -c '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" || true)"
  if [ "$status_count" -ne 2 ]; then
    cat "$TMPDIR/workflow.out"
    fail "expected two service-set phase status lines"
  fi

  pre_line="$(${pkgs.gnugrep}/bin/grep -n '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  body_line="$(${pkgs.gnugrep}/bin/grep -n '^BODY$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  post_line="$(${pkgs.gnugrep}/bin/grep -n '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/tail -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"

  if [ -z "$pre_line" ] || [ -z "$body_line" ] || [ -z "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "missing expected workflow phase markers"
  fi

  if [ "$pre_line" -ge "$body_line" ] || [ "$body_line" -ge "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set adapters did not preserve phase ordering"
  fi

  echo "OK: service-set export, introspection, and workflow adapter behavior stay aligned" > "$out"
''
