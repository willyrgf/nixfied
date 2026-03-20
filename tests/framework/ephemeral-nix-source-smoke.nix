{
  pkgs,
  model,
  services,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";
  sourceFixture = ./fixtures/ephemeral-nix-source-fixture;

  taskId = "task.test.ephemeral.nix-source";
  workflowId = "workflow.test.ephemeral.nix-source";

  probeModel = model // {
    tasks = model.tasks // {
      ${taskId} = baseTask // {
        id = taskId;
        summary = "ephemeral nix-source probe";
        description = "Verifies ephemeral source materialization uses the compiled source snapshot by default.";
        runner = {
          type = "shell";
          command = ''
            set -euo pipefail

            if [ "$(cat source-origin.txt)" != "store-snapshot" ]; then
              echo "expected source-origin.txt from compiled project snapshot"
              exit 1
            fi

            if [ ! -f kept.txt ]; then
              echo "expected kept.txt in compiled project snapshot"
              exit 1
            fi

            if [ -e runtime-only.txt ]; then
              echo "runtime caller file leaked into nix-source copy"
              exit 1
            fi

            if [ -e node_modules/ignored.txt ]; then
              echo "excluded node_modules content leaked into nix-source copy"
              exit 1
            fi

            if [ -e build.log ]; then
              echo "excluded build.log leaked into nix-source copy"
              exit 1
            fi

            echo "OK: nix-source probe complete"
          '';
          package = null;
          workflowId = null;
        };
        ui = baseTask.ui // {
          app = baseTask.ui.app // {
            expose = false;
            name = "test-ephemeral-nix-source";
          };
        };
      };
    };
    workflows = model.workflows // {
      ${workflowId} = {
        id = workflowId;
        summary = "ephemeral nix-source probe workflow";
        description = "Validates deterministic source materialization in ephemeral mode.";
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
    };
  };

  orchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit services;
    projectRoot = sourceFixture;
  };
in
pkgs.runCommand "ephemeral-nix-source-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'caller-root\n' > "$repo/source-origin.txt"
  printf 'runtime-only\n' > "$repo/runtime-only.txt"

  out_file="$TMPDIR/nix-source.out"
  set +e
  NIXFIED_CALLER_PWD="$repo/subdir" "$ORCH" run-workflow "${workflowId}" --summary > "$out_file" 2>&1
  rc="$?"
  set -e
  if [ "$rc" -ne 0 ]; then
    echo "probe workflow failed rc=$rc"
    cat "$out_file"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using nix-source copy mode" "$out_file"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: nix-source probe complete" "$out_file"

  echo "OK: ephemeral nix-source mode uses the compiled source snapshot by default" > "$out"
''
