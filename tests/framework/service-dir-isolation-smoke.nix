{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.service-dir.probe";
  probeWorkflowId = "workflow.test.service-dir.probe";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "service dir probe";
    description = "Prints exported service dir env vars for validation.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: postgres_data=$NIXFIED_SERVICE_POSTGRES_DATA_DIR"
        echo "INFO: postgres_state=$NIXFIED_SERVICE_POSTGRES_STATE_DIR"
        echo "INFO: postgres_log=$NIXFIED_SERVICE_POSTGRES_LOG_DIR"
        echo "INFO: nginx_data=$NIXFIED_SERVICE_NGINX_DATA_DIR"
        echo "INFO: nginx_state=$NIXFIED_SERVICE_NGINX_STATE_DIR"
        echo "INFO: nginx_log=$NIXFIED_SERVICE_NGINX_LOG_DIR"
        echo "OK: service dir probe complete"
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-service-dir-probe";
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
    summary = "service dir probe workflow";
    description = "Runs the service dir probe in ephemeral mode.";
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

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "service-dir-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"
  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"

  non_ephemeral_scope="$TMPDIR/non-ephemeral-runtime"
  PROJECT_ENV=test NIX_ENV=2 REGISTRY_ROOT="$TMPDIR/non-ephemeral-registry" CI_ARTIFACTS_DIR="$TMPDIR/non-ephemeral-artifacts" \
    NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$non_ephemeral_scope" \
    "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/non-ephemeral.out" 2>&1

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_data=$non_ephemeral_scope/services/postgres/data" "$TMPDIR/non-ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_state=$non_ephemeral_scope/services/postgres/state" "$TMPDIR/non-ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_log=$non_ephemeral_scope/services/postgres/log" "$TMPDIR/non-ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_data=$non_ephemeral_scope/services/nginx/data" "$TMPDIR/non-ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_state=$non_ephemeral_scope/services/nginx/state" "$TMPDIR/non-ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_log=$non_ephemeral_scope/services/nginx/log" "$TMPDIR/non-ephemeral.out"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked service dir probe\n' > "$repo/tracked.txt"
  printf 'tracked subdir probe\n' > "$repo/subdir/probe.txt"
  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/probe.txt
  ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -m "init service dir probe repo" >/dev/null 2>&1

  REGISTRY_ROOT="$TMPDIR/host-registry" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow "${probeWorkflowId}" --summary > "$TMPDIR/ephemeral.out" 2>&1
  eph_root="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/ephemeral.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$eph_root" "eph_root"

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_data=$eph_root/services/postgres/data" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_state=$eph_root/services/postgres/state" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: postgres_log=$eph_root/services/postgres/log" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_data=$eph_root/services/nginx/data" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_state=$eph_root/services/nginx/state" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: nginx_log=$eph_root/services/nginx/log" "$TMPDIR/ephemeral.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: service dir probe complete" "$TMPDIR/ephemeral.out"

  echo "OK: service dir env vars are correctly scoped" > "$out"
''
