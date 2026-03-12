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
        echo "INFO: sandbox_pwd=$(pwd -P)"
        if [ ! -f "./tracked.txt" ]; then
          echo "missing tracked file in ephemeral source copy"
          exit 1
        fi
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
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
      };
    };
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
    workflows = model.workflows // {
      ${probeWorkflowId} = probeWorkflow;
    };
  };

  orchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
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
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked\n' > "$repo/tracked.txt"
  printf 'subdir tracked\n' > "$repo/subdir/tracked-subdir.txt"

  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" config user.name "nixfied tests"
  ${pkgs.git}/bin/git -C "$repo" config user.email "nixfied-tests@example.invalid"
  ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/tracked-subdir.txt
  ${pkgs.git}/bin/git -C "$repo" commit -m "seed tracked files" >/dev/null 2>&1

  NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow "${probeWorkflowId}" --summary-file "$TMPDIR/probe.summary.json" --summary > "$TMPDIR/probe.out" 2>&1

  eph_root="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ -z "$eph_root" ]; then
    echo "missing ephemeral root"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ "$sandbox_pwd" != "$eph_root/source" ]; then
    echo "expected task workdir inside ephemeral source copy"
    echo "sandbox_pwd=$sandbox_pwd"
    echo "eph_root=$eph_root"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Ephemeral execution mode" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: ephemeral probe task complete" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: Ephemeral state cleaned" "$TMPDIR/probe.out"
  if [ ! -f "$TMPDIR/probe.summary.json" ]; then
    echo "missing summary file"
    cat "$TMPDIR/probe.out"
    exit 1
  fi
  ${pkgs.jq}/bin/jq -e '.workflow_id == "'"${probeWorkflowId}"'"' "$TMPDIR/probe.summary.json" > /dev/null

  if ${pkgs.gnugrep}/bin/grep -Fq "Permission denied" "$TMPDIR/probe.out"; then
    echo "unexpected permission error during ephemeral cleanup"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  if [ -e "$eph_root" ]; then
    echo "expected ephemeral root to be cleaned: $eph_root"
    exit 1
  fi

  echo "OK: ephemeral execution uses copied writable source root" > "$out"
''
