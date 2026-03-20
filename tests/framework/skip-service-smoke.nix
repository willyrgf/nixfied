{
  pkgs,
  registry,
}:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied.services.postgres.enable = lib.mkForce true;
        }
      )
    ];
    localOverrides = [ ];
  };
  baseTask = compiled.model.tasks."task.check";

  mkShellTask =
    {
      id,
      requiredServices ? [ ],
      commandTail,
    }:
    baseTask
    // {
      inherit id;
      requirements = (baseTask.requirements or { }) // {
        services = requiredServices;
      };
      summary = id;
      description = id;
      runner = {
        type = "shell";
        command = commandTail;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
      ui = baseTask.ui // {
        app = baseTask.ui.app // {
          expose = false;
          name = builtins.replaceStrings [ "." ] [ "-" ] id;
        };
      };
    };

  taskSkipId = "task.test.skip.truthy";
  controlTaskId = "task.test.skip.truthy.control";
  dependencyTaskId = "task.test.skip.workflow.dependency";
  consumerTaskId = "task.test.skip.workflow.consumer";
  dependencyWorkflowId = "workflow.test.skip.dependency";
  skipService = "postgres";
  skipEnvVar = "SKIP_${lib.toUpper skipService}";

  probeModel = compiled.model // {
    serviceCatalog = compiled.model.serviceCatalog // {
      "service.postgres" = compiled.model.serviceCatalog."service.postgres" // {
        enable = true;
      };
    };

    tasks = compiled.model.tasks // {
      "${taskSkipId}" = mkShellTask {
        id = taskSkipId;
        requiredServices = [ skipService ];
        commandTail = ''
          set -euo pipefail
          printf '%s\n' "skip-task-main-ran"
        '';
      };

      "${controlTaskId}" = mkShellTask {
        id = controlTaskId;
        commandTail = ''
          set -euo pipefail
          printf '%s\n' "control-task-ran"
        '';
      };

      "${dependencyTaskId}" = mkShellTask {
        id = dependencyTaskId;
        requiredServices = [ skipService ];
        commandTail = ''
          set -euo pipefail
          printf '%s\n' "dependency-task-ran"
        '';
      };

      "${consumerTaskId}" = mkShellTask {
        id = consumerTaskId;
        commandTail = ''
          set -euo pipefail
          printf '%s\n' "consumer-task-ran"
        '';
      };
    };

    workflows = compiled.model.workflows // {
      "${dependencyWorkflowId}" = {
        id = dependencyWorkflowId;
        summary = dependencyWorkflowId;
        description = "Validate skipped-service hard-fail dependency behavior";
        mode = "custom";
        maxWorkers = 1;
        units = {
          "${dependencyTaskId}" = {
            taskId = dependencyTaskId;
            needs = [ ];
            locks = [ ];
            requirements.services = [ skipService ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
          "${consumerTaskId}" = {
            taskId = consumerTaskId;
            needs = [ "${dependencyTaskId}" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          };
        };
        stages = [
          [ "${dependencyTaskId}" ]
          [ "${consumerTaskId}" ]
        ];
        preRun = {
          tasks = [ ];
        };
        postRun = {
          tasks = [ ];
          alwaysRun = false;
        };
        artifacts = {
          root = "artifacts-root";
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = false;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral = {
            enable = true;
          };
        };
        plan = [
          {
            name = "${dependencyTaskId}";
            taskId = dependencyTaskId;
            needs = [ ];
            locks = [ ];
            requirements.services = [ skipService ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          }
          {
            name = "${consumerTaskId}";
            taskId = consumerTaskId;
            needs = [ "${dependencyTaskId}" ];
            locks = [ ];
            when = {
              envEquals = { };
              envPresent = [ ];
            };
            skipIfMissingEnv = [ ];
          }
        ];
      };
    };
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    services = compiled.services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "skip-service-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  REGISTRY_FILE="$TMPDIR/registry/events.ndjson"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  for value in 1 true TRUE yes on; do
    set +e
    ${skipEnvVar}="$value" "$EXECUTOR" run-task "${taskSkipId}" > "$TMPDIR/task-''${value}.out" 2>&1
    task_rc="$?"
    set -e
    if [ "$task_rc" -ne 0 ]; then
      echo "expected run-task skip with ${skipEnvVar}=$value to pass, rc=$task_rc"
      cat "$TMPDIR/task-''${value}.out"
      exit 1
    fi
    if ${pkgs.gnugrep}/bin/grep -Fq -- "skip-task-main-ran" "$TMPDIR/task-''${value}.out"; then
      echo "expected run-task skip with ${skipEnvVar}=$value to skip command execution"
      cat "$TMPDIR/task-''${value}.out"
      exit 1
    fi
  require_contains "$TMPDIR/task-''${value}.out" "SKIP: task '${taskSkipId}' is skipped because service '${skipService}' has a skip flag enabled"
  done

  "$EXECUTOR" run-task "${controlTaskId}" > "$TMPDIR/task-control.out" 2>&1
  if ! ${pkgs.gnugrep}/bin/grep -Fq -- "control-task-ran" "$TMPDIR/task-control.out"; then
    echo "expected non-skipped run-task to execute command"
    cat "$TMPDIR/task-control.out"
    exit 1
  fi

  set +e
  SKIP_POSTGRES=on "$EXECUTOR" run-task task.ops.health --service postgres > "$TMPDIR/ops-health.out" 2>&1
  ops_health_rc="$?"
  set -e
  if [ "$ops_health_rc" -ne 0 ]; then
    echo "expected ops health to no-op when skipped service is requested, rc=$ops_health_rc"
    cat "$TMPDIR/ops-health.out"
    exit 1
  fi
  require_contains "$TMPDIR/ops-health.out" "SKIP: no enabled services for health checks"
  if ${pkgs.gnugrep}/bin/grep -Fq "ERROR:" "$TMPDIR/ops-health.out"; then
    echo "unexpected error output in skipped ops health path"
    cat "$TMPDIR/ops-health.out"
    exit 1
  fi

  dependency_skip_run_id_file="$TMPDIR/dependency-skip.run-id"
  set +e
  ${skipEnvVar}=on \
    "$EXECUTOR" run-workflow "${dependencyWorkflowId}" --run-id-file "$dependency_skip_run_id_file" > "$TMPDIR/dependency-skip.out" 2>&1
  dependency_skip_rc="$?"
  set -e
  if [ "$dependency_skip_rc" -ne 0 ]; then
    echo "expected skip-only workflow to exit 0, got $dependency_skip_rc"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi
  if ${pkgs.gnugrep}/bin/grep -Fq -- "dependency-task-ran" "$TMPDIR/dependency-skip.out" \
    || ${pkgs.gnugrep}/bin/grep -Fq -- "consumer-task-ran" "$TMPDIR/dependency-skip.out"; then
    echo "expected skipped dependency workflow units not to run"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi

  skip_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$dependency_skip_run_id_file")"
  if [ -z "$skip_run_id" ]; then
    echo "missing dependency skip workflow run id"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi

  dependency_skip_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$skip_run_id" --arg taskId "${dependencyTaskId}" '
    select(.runId == $runId and .taskId == $taskId and .state == "canceled") | .detail.reason
  ' "$REGISTRY_FILE" | ${pkgs.coreutils}/bin/head -n 1)"
  consumer_skip_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$skip_run_id" --arg taskId "${consumerTaskId}" '
    select(.runId == $runId and .taskId == $taskId and .state == "canceled") | .detail.reason
  ' "$REGISTRY_FILE" | ${pkgs.coreutils}/bin/head -n 1)"
  consumer_skip_dependency="$(${pkgs.jq}/bin/jq -r --arg runId "$skip_run_id" --arg taskId "${consumerTaskId}" '
    select(.runId == $runId and .taskId == $taskId and .state == "canceled") | .detail.dependency
  ' "$REGISTRY_FILE" | ${pkgs.coreutils}/bin/head -n 1)"

  if [ "$dependency_skip_reason" != "service-skipped" ]; then
    echo "expected dependency task skip reason service-skipped, got $dependency_skip_reason"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi
  if [ "$consumer_skip_reason" != "dependency-skipped" ]; then
    echo "expected consumer task skip reason dependency-skipped, got $consumer_skip_reason"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi
  if [ "$consumer_skip_dependency" != "${dependencyTaskId}" ]; then
    echo "expected consumer dependency reference ${dependencyTaskId}, got $consumer_skip_dependency"
    cat "$TMPDIR/dependency-skip.out"
    exit 1
  fi

  # Validate summary.json for skip-only workflow
  skip_summary_file="$CI_ARTIFACTS_ROOT/$skip_run_id/summary.json"
  if [ ! -f "$skip_summary_file" ]; then
    # Try fallback locations
    skip_summary_file="$(find "$CI_ARTIFACTS_ROOT" -name summary.json -path "*$skip_run_id*" 2>/dev/null | head -n 1 || true)"
  fi
  if [ -n "$skip_summary_file" ] && [ -f "$skip_summary_file" ]; then
    summary_exit_code="$(${pkgs.jq}/bin/jq -r '.exit_code' "$skip_summary_file")"
    summary_skipped="$(${pkgs.jq}/bin/jq -r '.counts.skipped' "$skip_summary_file")"
    summary_canceled="$(${pkgs.jq}/bin/jq -r '.counts.canceled' "$skip_summary_file")"

    if [ "$summary_exit_code" != "0" ]; then
      echo "expected summary.json exit_code 0, got $summary_exit_code"
      ${pkgs.jq}/bin/jq . "$skip_summary_file"
      exit 1
    fi
    if [ "$summary_skipped" != "2" ]; then
      echo "expected summary.json counts.skipped 2, got $summary_skipped"
      ${pkgs.jq}/bin/jq . "$skip_summary_file"
      exit 1
    fi
    if [ "$summary_canceled" != "0" ]; then
      echo "expected summary.json counts.canceled 0, got $summary_canceled"
      ${pkgs.jq}/bin/jq . "$skip_summary_file"
      exit 1
    fi

    dep_step_status="$(${pkgs.jq}/bin/jq -r --arg taskId "${dependencyTaskId}" '.steps[] | select(.name == $taskId) | .status' "$skip_summary_file")"
    con_step_status="$(${pkgs.jq}/bin/jq -r --arg taskId "${consumerTaskId}" '.steps[] | select(.name == $taskId) | .status' "$skip_summary_file")"
    dep_step_reason="$(${pkgs.jq}/bin/jq -r --arg taskId "${dependencyTaskId}" '.steps[] | select(.name == $taskId) | .reason' "$skip_summary_file")"
    con_step_reason="$(${pkgs.jq}/bin/jq -r --arg taskId "${consumerTaskId}" '.steps[] | select(.name == $taskId) | .reason' "$skip_summary_file")"

    if [ "$dep_step_status" != "skipped" ]; then
      echo "expected dependency step status skipped, got $dep_step_status"
      exit 1
    fi
    if [ "$con_step_status" != "skipped" ]; then
      echo "expected consumer step status skipped, got $con_step_status"
      exit 1
    fi
    if [ "$dep_step_reason" != "service-skipped" ]; then
      echo "expected dependency step reason service-skipped, got $dep_step_reason"
      exit 1
    fi
    if [ "$con_step_reason" != "dependency-skipped" ]; then
      echo "expected consumer step reason dependency-skipped, got $con_step_reason"
      exit 1
    fi
  fi

  dependency_run_id_file="$TMPDIR/dependency-ok.run-id"
  "$EXECUTOR" run-workflow "${dependencyWorkflowId}" \
    --run-id-file "$dependency_run_id_file" > "$TMPDIR/dependency-ok.out" 2>&1
  if ! ${pkgs.gnugrep}/bin/grep -Fq -- "dependency-task-ran" "$TMPDIR/dependency-ok.out" \
    || ! ${pkgs.gnugrep}/bin/grep -Fq -- "consumer-task-ran" "$TMPDIR/dependency-ok.out"; then
    echo "expected dependency workflow units to execute when not skipped"
    cat "$TMPDIR/dependency-ok.out"
    exit 1
  fi

  echo "OK: skip truthy, operations, dependency cascade, and summary behavior are validated" > "$out"
''
