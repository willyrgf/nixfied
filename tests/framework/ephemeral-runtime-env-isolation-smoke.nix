{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.ephemeral.runtime-env";
  probeWorkflowId = "workflow.test.ephemeral.runtime-env";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "ephemeral runtime env probe";
    description = "Verifies ephemeral runs receive only run-local mutable directories.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_pwd=$(pwd -P)"
        if [ ! -f "./tracked.txt" ]; then
          echo "missing tracked file in ephemeral source copy"
          exit 1
        fi
        echo "INFO: sandbox_home=$HOME"
        echo "INFO: sandbox_tmp=$TMPDIR"
        echo "INFO: sandbox_xdg_data=$XDG_DATA_HOME"
        echo "INFO: sandbox_xdg_state=$XDG_STATE_HOME"
        echo "INFO: sandbox_xdg_cache=$XDG_CACHE_HOME"
        echo "INFO: sandbox_registry=$REGISTRY_ROOT"
        echo "INFO: sandbox_artifacts=$CI_ARTIFACTS_DIR"
        echo "OK: ephemeral runtime env probe complete"
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-ephemeral-runtime-env";
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
    summary = "ephemeral runtime env probe workflow";
    description = "Validates the ephemeral runtime env contract.";
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

  orchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-runtime-env-isolation-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  hostile_runtime_root="$TMPDIR/hostile-runtime"
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

  set +e
  NIXFIED_CALLER_PWD="$repo/subdir" \
  NIXFIED_RUNTIME_HOME="$hostile_runtime_root/home" \
  NIXFIED_RUNTIME_TMPDIR="$hostile_runtime_root/tmp" \
  NIXFIED_RUNTIME_XDG_DATA_HOME="$hostile_runtime_root/xdg/data" \
  NIXFIED_RUNTIME_XDG_STATE_HOME="$hostile_runtime_root/xdg/state" \
  NIXFIED_RUNTIME_XDG_CACHE_HOME="$hostile_runtime_root/xdg/cache" \
  NIXFIED_RUNTIME_REGISTRY_ROOT="$hostile_runtime_root/registry" \
  NIXFIED_RUNTIME_ARTIFACTS_DIR="$hostile_runtime_root/artifacts" \
  NIXFIED_RUNTIME_SERVICE_ROOT="$hostile_runtime_root/services" \
    "$ORCH" run-workflow "${probeWorkflowId}" --summary > "$TMPDIR/probe.out" 2>&1
  probe_rc="$?"
  set -e
  if [ "$probe_rc" -ne 0 ]; then
    echo "probe workflow failed rc=$probe_rc"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  eph_root="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ -z "$eph_root" ]; then
    echo "missing ephemeral root"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_pwd=$eph_root/source" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_home=$eph_root/home" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_tmp=$eph_root/tmp" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_data=$eph_root/xdg/data" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_state=$eph_root/xdg/state" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_cache=$eph_root/xdg/cache" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_registry=$eph_root/registry" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_artifacts=$eph_root/artifacts" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: ephemeral runtime env probe complete" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: Ephemeral state cleaned" "$TMPDIR/probe.out"
  if ${pkgs.gnugrep}/bin/grep -Fq "$hostile_runtime_root" "$TMPDIR/probe.out"; then
    echo "ephemeral runtime env should ignore hostile runtime override inputs"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  echo "OK: ephemeral runtime env vars are run-local" > "$out"
''
