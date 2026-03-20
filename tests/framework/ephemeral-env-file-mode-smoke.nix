{
  pkgs,
  model,
  services,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";
  envFileName = ".ephemeral-secret.env";

  mkProbeTask =
    {
      taskId,
      appName,
      expectSecret,
    }:
    baseTask
    // {
      id = taskId;
      summary = "ephemeral env file mode probe";
      description = "Verifies host env file import is opt-in in ephemeral mode.";
      runner = {
        type = "shell";
        command = ''
          set -euo pipefail
          echo "INFO: env_secret=''${EPHEMERAL_HOST_ENV_SECRET:-}"

          if [ "${if expectSecret then "1" else "0"}" = "1" ]; then
            if [ "''${EPHEMERAL_HOST_ENV_SECRET:-}" != "from-host-env" ]; then
              echo "expected host env secret to be imported"
              exit 1
            fi
          else
            if [ -n "''${EPHEMERAL_HOST_ENV_SECRET:-}" ]; then
              echo "host env secret should not be imported in reproducible mode"
              exit 1
            fi
          fi

          echo "OK: ephemeral env file probe complete expect_secret=${if expectSecret then "1" else "0"}"
        '';
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "EPHEMERAL_HOST_ENV_SECRET" ];
        allowSensitivePassThrough = true;
      };
      ui = baseTask.ui // {
        app = baseTask.ui.app // {
          expose = false;
          name = appName;
        };
      };
    };

  mkProbeWorkflow =
    {
      workflowId,
      taskId,
    }:
    {
      id = workflowId;
      summary = "ephemeral env file mode probe workflow";
      description = "Validates reproducible default env loading and explicit host env opt-in.";
      mode = "custom";
      maxWorkers = 1;
      units = {
        probe = {
          inherit taskId;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        };
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
          name = "probe";
          inherit taskId;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        }
      ];
    };

  disabledTaskId = "task.test.ephemeral.env-file.disabled";
  disabledWorkflowId = "workflow.test.ephemeral.env-file.disabled";
  originalRootTaskId = "task.test.ephemeral.env-file.original-root";
  originalRootWorkflowId = "workflow.test.ephemeral.env-file.original-root";

  mkEphemeralModel =
    {
      envFileMode,
      taskId,
      appName,
      workflowId,
      expectSecret,
    }:
    model
    // {
      runtime = model.runtime // {
        ephemeral = (model.runtime.ephemeral or { }) // {
          copyMode = "git-files";
          includeUntracked = false;
          inherit envFileMode;
          envFilePath = envFileName;
        };
      };
      tasks = model.tasks // {
        ${taskId} = mkProbeTask {
          inherit
            taskId
            appName
            expectSecret
            ;
        };
      };
      workflows = model.workflows // {
        ${workflowId} = mkProbeWorkflow {
          inherit
            workflowId
            taskId
            ;
        };
      };
    };

  disabledModel = mkEphemeralModel {
    envFileMode = "disabled";
    taskId = disabledTaskId;
    appName = "test-ephemeral-env-file-disabled";
    workflowId = disabledWorkflowId;
    expectSecret = false;
  };

  originalRootModel = mkEphemeralModel {
    envFileMode = "original-root";
    taskId = originalRootTaskId;
    appName = "test-ephemeral-env-file-original-root";
    workflowId = originalRootWorkflowId;
    expectSecret = true;
  };

  disabledOrchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = disabledModel;
    inherit services;
    projectRoot = ../..;
  };

  originalRootOrchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = originalRootModel;
    inherit services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-env-file-mode-smoke" { } ''
  set -euo pipefail

  DISABLED_ORCH="${disabledOrchestrator}/bin/nixfied-orchestrator"
  ORIGINAL_ROOT_ORCH="${originalRootOrchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked\n' > "$repo/README.md"
  printf 'EPHEMERAL_HOST_ENV_SECRET=from-host-env\n' > "$repo/${envFileName}"

  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add README.md

  run_probe() {
    local label="$1"
    local orch="$2"
    local workflow_id="$3"
    local out_file="$TMPDIR/$label.out"
    local rc

    set +e
    NIXFIED_CALLER_PWD="$repo/subdir" "$orch" run-workflow "$workflow_id" --summary > "$out_file" 2>&1
    rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "probe workflow failed label=$label rc=$rc"
      cat "$out_file"
      exit 1
    fi
  }

  run_probe disabled "$DISABLED_ORCH" "${disabledWorkflowId}"
  run_probe original-root "$ORIGINAL_ROOT_ORCH" "${originalRootWorkflowId}"

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Skipping host env file import mode=disabled" "$TMPDIR/disabled.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: env_secret=" "$TMPDIR/disabled.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: ephemeral env file probe complete expect_secret=0" "$TMPDIR/disabled.out"

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Loading host env file mode=original-root path=$repo/${envFileName}" "$TMPDIR/original-root.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: env_secret=from-host-env" "$TMPDIR/original-root.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: ephemeral env file probe complete expect_secret=1" "$TMPDIR/original-root.out"

  echo "OK: ephemeral env file import is reproducible by default and opt-in when requested" > "$out"
''
