{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.ephemeral.probe";
  probeWorkflowId = "workflow.test.ephemeral.probe";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "ephemeral execution probe";
    description = "Verifies workflow execution uses a writable ephemeral source copy.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        pwd -P > "$CI_ARTIFACTS_DIR/workdir.txt"
        touch "./ephemeral-write-check.txt"
        mkdir -p "./result/bin"
        printf 'probe\n' > "./result/bin/mfm_cli"
        chmod -R a-w "./result"
        echo "OK: ephemeral probe task complete"
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-ephemeral-probe";
      };
    };
  };

  probeUnit = {
    taskId = probeTaskId;
    needs = [ ];
    locks = [ ];
    when = {
      envEquals = { };
      envPresent = [ ];
    };
    skipIfMissingEnv = [ ];
  };

  probeWorkflow = {
    id = probeWorkflowId;
    summary = "ephemeral execution probe workflow";
    description = "Validates orchestrator-enforced filesystem isolation.";
    mode = "custom";
    maxWorkers = 1;
    units = {
      probe = probeUnit;
    };
    stages = [ [ "probe" ] ];
    preRun = {
      tasks = [ ];
    };
    postRun = {
      tasks = [ ];
      alwaysRun = true;
    };
    artifacts = {
      root = "/tmp/ci-artifacts";
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
          name = "probe";
        }
        // probeUnit
      )
    ];
  };

  probeModel = model // {
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
    workflows = model.workflows // {
      ${probeWorkflowId} = probeWorkflow;
    };
  };

  orchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-execution-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"

  "$ORCH" run-workflow "${probeWorkflowId}" --summary > "$TMPDIR/probe.out" 2>&1

  workdir_file="$CI_ARTIFACTS_DIR/workdir.txt"
  if [ ! -f "$workdir_file" ]; then
    echo "missing probe workdir output: $workdir_file"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Eq '.+-ephemeral-.+/source$' "$workdir_file"; then
    echo "expected task workdir inside ephemeral source copy"
    cat "$workdir_file"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Ephemeral execution mode" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: ephemeral probe task complete" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: Ephemeral state cleaned" "$TMPDIR/probe.out"

  if ${pkgs.gnugrep}/bin/grep -Fq "Permission denied" "$TMPDIR/probe.out"; then
    echo "unexpected permission error during ephemeral cleanup"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  summary_file="$CI_ARTIFACTS_DIR/summary.json"
  if [ ! -f "$summary_file" ]; then
    echo "missing summary file: $summary_file"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.jq}/bin/jq -e '
    (.timing.setup_duration | type) == "number"
    and (.timing.total_duration | type) == "number"
    and (.timing.total_duration >= .timing.setup_duration)
  ' "$summary_file" > /dev/null

  echo "OK: ephemeral execution uses copied writable source root" > "$out"
''
