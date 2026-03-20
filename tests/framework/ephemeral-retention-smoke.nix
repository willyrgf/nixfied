{
  pkgs,
  model,
  services,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  failTaskId = "task.test.ephemeral.retention.fail";
  failWorkflowId = "workflow.test.ephemeral.retention.fail";

  failTask = baseTask // {
    id = failTaskId;
    summary = "ephemeral retention fail probe";
    description = "Forces failure to test failed-root retention and pruning.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "ERROR: intentional failure for retention probe" >&2
        exit 1
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-ephemeral-retention-fail";
      };
    };
  };

  failUnit = {
    taskId = failTaskId;
    needs = [ ];
    locks = [ ];
    when = {
      envEquals = { };
      envPresent = [ ];
    };
    skipIfMissingEnv = [ ];
  };

  failWorkflow = {
    id = failWorkflowId;
    summary = "ephemeral retention fail workflow";
    description = "Validates failed-root retention limits for ephemeral runs.";
    mode = "custom";
    maxWorkers = 1;
    units = {
      fail = failUnit;
    };
    stages = [ [ "fail" ] ];
    preRun = {
      tasks = [ ];
    };
    postRun = {
      tasks = [ ];
      alwaysRun = true;
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
      (
        {
          name = "fail";
        }
        // failUnit
      )
    ];
  };

  withRetentionModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "static-excludes";
        keepFailures = true;
        maxFailedRoots = 2;
        maxFailedRootAgeHours = 1;
      };
    };
    tasks = model.tasks // {
      ${failTaskId} = failTask;
    };
    workflows = model.workflows // {
      ${failWorkflowId} = failWorkflow;
    };
  };

  noRetentionModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "static-excludes";
        keepFailures = false;
        maxFailedRoots = 0;
        maxFailedRootAgeHours = 0;
      };
    };
    tasks = model.tasks // {
      ${failTaskId} = failTask;
    };
    workflows = model.workflows // {
      ${failWorkflowId} = failWorkflow;
    };
  };

  orchestratorKeep = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = withRetentionModel;
    inherit services;
    projectRoot = ../..;
  };

  orchestratorDrop = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = noRetentionModel;
    inherit services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-retention-smoke" { } ''
  set -euo pipefail

  ORCH_KEEP="${orchestratorKeep}/bin/nixfied-orchestrator"
  ORCH_DROP="${orchestratorDrop}/bin/nixfied-orchestrator"
  project_id="${model.identity.projectId}"

  run_expected_failure() {
    local label="$1"
    local orch="$2"
    local root_base="$3"
    local log_file="$4"
    export REGISTRY_ROOT="$root_base/registry-$label"
    export CI_ARTIFACTS_DIR="$root_base/artifacts-$label"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"
    set +e
    NIXFIED_EPHEMERAL_ROOT_BASE="$root_base" "$orch" run-workflow "${failWorkflowId}" --summary > "$log_file" 2>&1
    local rc=$?
    set -e
    if [ "$rc" -eq 0 ]; then
      echo "expected failure for retention probe"
      cat "$log_file"
      exit 1
    fi
  }

  keep_base="$TMPDIR/keep-base"
  mkdir -p "$keep_base"
  old_failed="$keep_base/$project_id-ephemeral-failed-19990101-000000-slot0-old"
  mkdir -p "$old_failed"
  touch -t 199901010000 "$old_failed"

  run_expected_failure "keep1" "$ORCH_KEEP" "$keep_base" "$TMPDIR/keep1.out"
  if [ -d "$old_failed" ]; then
    echo "old failed root was not pruned by age"
    find "$keep_base" -maxdepth 1 -mindepth 1 -type d | sort
    exit 1
  fi

  run_expected_failure "keep2" "$ORCH_KEEP" "$keep_base" "$TMPDIR/keep2.out"
  run_expected_failure "keep3" "$ORCH_KEEP" "$keep_base" "$TMPDIR/keep3.out"

  mapfile -t kept_failed < <(${pkgs.findutils}/bin/find "$keep_base" -maxdepth 1 -mindepth 1 -type d -name "$project_id-ephemeral-failed-*" | sort)
  if [ "''${#kept_failed[@]}" -ne 2 ]; then
    echo "expected exactly 2 retained failed roots, got ''${#kept_failed[@]}"
    find "$keep_base" -maxdepth 1 -mindepth 1 -type d | sort
    exit 1
  fi

  drop_base="$TMPDIR/drop-base"
  mkdir -p "$drop_base"
  run_expected_failure "drop1" "$ORCH_DROP" "$drop_base" "$TMPDIR/drop1.out"

  if ${pkgs.findutils}/bin/find "$drop_base" -maxdepth 1 -mindepth 1 -type d -name "$project_id-ephemeral-failed-*" | ${pkgs.gnugrep}/bin/grep -q .; then
    echo "failed roots should not be retained when keepFailures=false"
    find "$drop_base" -maxdepth 1 -mindepth 1 -type d | sort
    exit 1
  fi

  echo "OK: ephemeral failed-root retention is bounded and configurable" > "$out"
''
