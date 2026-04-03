{
  pkgs,
  model,
  services,
  registry,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  baseTask = model.tasks."task.ci.quality";

  okTaskId = "task.test.ephemeral.copy-budget.ok";
  okWorkflowId = "workflow.test.ephemeral.copy-budget.ok";

  okTask = baseTask // {
    id = okTaskId;
    summary = "ephemeral copy budget probe";
    description = "No-op task used to validate budget guardrails before execution.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "OK: budget task executed"
      '';
      package = null;
      workflowId = null;
    };
  };

  okUnit = {
    taskId = okTaskId;
    needs = [ ];
    locks = [ ];
    when = {
      envEquals = { };
      envPresent = [ ];
    };
    skipIfMissingEnv = [ ];
  };

  okWorkflow = {
    id = okWorkflowId;
    summary = "ephemeral copy budget workflow";
    description = "Validates pre-copy disk budget checks.";
    mode = "custom";
    maxWorkers = 1;
    units = {
      ok = okUnit;
    };
    stages = [ [ "ok" ] ];
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
          name = "ok";
        }
        // okUnit
      )
    ];
  };

  minFreeModel = runtimeFixture.withCompiledExecution (
    model
    // {
      runtime = model.runtime // {
        ephemeral = (model.runtime.ephemeral or { }) // {
          copyMode = "static-excludes";
          keepFailures = false;
          maxCopyBytes = 0;
          minFreeBytesAfterCopy = 9223372036854775807;
        };
      };
      tasks = model.tasks // {
        ${okTaskId} = okTask;
      };
      workflows = model.workflows // {
        ${okWorkflowId} = okWorkflow;
      };
    }
  );

  maxCopyModel = runtimeFixture.withCompiledExecution (
    model
    // {
      runtime = model.runtime // {
        ephemeral = (model.runtime.ephemeral or { }) // {
          copyMode = "static-excludes";
          keepFailures = false;
          maxCopyBytes = 1;
          minFreeBytesAfterCopy = 0;
        };
      };
      tasks = model.tasks // {
        ${okTaskId} = okTask;
      };
      workflows = model.workflows // {
        ${okWorkflowId} = okWorkflow;
      };
    }
  );

  orchestratorMinFree = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = minFreeModel;
    inherit services;
    projectRoot = ../..;
  };

  orchestratorMaxCopy = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = maxCopyModel;
    inherit services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-copy-budget-smoke" { } ''
  set -euo pipefail

  ORCH_MIN_FREE="${orchestratorMinFree}/bin/nixfied-orchestrator"
  ORCH_MAX_COPY="${orchestratorMaxCopy}/bin/nixfied-orchestrator"

  run_expect_budget_failure() {
    local label="$1"
    local orch="$2"
    local root_base="$3"
    local expected="$4"
    local log_file="$5"
    export REGISTRY_ROOT="$root_base/registry-$label"
    export CI_ARTIFACTS_DIR="$root_base/artifacts-$label"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"
    set +e
    NIXFIED_EPHEMERAL_ROOT_BASE="$root_base" "$orch" run-workflow "${okWorkflowId}" --summary > "$log_file" 2>&1
    local rc=$?
    set -e
    if [ "$rc" -eq 0 ]; then
      echo "expected copy budget failure for $label"
      cat "$log_file"
      exit 1
    fi
    ${pkgs.gnugrep}/bin/grep -Fq "$expected" "$log_file"
  }

  min_free_base="$TMPDIR/min-free-base"
  max_copy_base="$TMPDIR/max-copy-base"
  mkdir -p "$min_free_base" "$max_copy_base"

  run_expect_budget_failure \
    "min-free" \
    "$ORCH_MIN_FREE" \
    "$min_free_base" \
    "min_free_after_copy=" \
    "$TMPDIR/min-free.out"

  run_expect_budget_failure \
    "max-copy" \
    "$ORCH_MAX_COPY" \
    "$max_copy_base" \
    "max_copy_bytes=" \
    "$TMPDIR/max-copy.out"

  ${pkgs.gnugrep}/bin/grep -Fq "ERROR: ephemeral copy budget exceeded" "$TMPDIR/min-free.out"
  ${pkgs.gnugrep}/bin/grep -Fq "ERROR: ephemeral copy budget exceeded" "$TMPDIR/max-copy.out"

  echo "OK: ephemeral copy budget guards fail fast" > "$out"
''
