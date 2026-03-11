{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.nix-checks.parent-workflow-skip";
  probeWorkflowId = "workflow.test.nix-checks.parent-workflow-skip";

  fakeFormatter = pkgs.writeShellScriptBin "nixfmt" ''
    set -euo pipefail
    if [ "$#" -lt 1 ] || [ "$1" != "--check" ]; then
      echo "unexpected nixfmt args: $*" >&2
      exit 1
    fi
  '';

  fakeNix = pkgs.writeShellScriptBin "nix" ''
    set -euo pipefail

    if [ -z "''${FAKE_NIX_LOG:-}" ]; then
      echo "FAKE_NIX_LOG is required" >&2
      exit 1
    fi

    printf '%s\n' "$*" >> "$FAKE_NIX_LOG"

    case "$1" in
      flake)
        case "''${2:-}" in
          show|check)
            exit 0
            ;;
          *)
            echo "unexpected flake subcommand: $*" >&2
            exit 1
            ;;
        esac
        ;;
      run)
        exit 0
        ;;
      *)
        echo "unexpected nix invocation: $*" >&2
        exit 1
        ;;
    esac
  '';

  fakePkgs = pkgs // {
    nix = fakeNix;
  };

  fakeNixChecksPkg =
    import ../../nixfied/lib/mkNixChecks.nix
      {
        pkgs = fakePkgs;
        lib = pkgs.lib;
      }
      {
        formatterPkg = fakeFormatter;
      };

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "nix checks parent workflow skip probe";
    description = "Captures fake nix invocations to prove nested workflow quality skips flake checks.";
    runner = baseTask.runner // {
      package = fakeNixChecksPkg;
      command = "nix-checks --mode full";
    };
    runtime = (baseTask.runtime or { }) // {
      passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "FAKE_NIX_LOG" ];
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-nix-checks-parent-workflow-skip";
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
    summary = "nix checks parent workflow skip probe";
    description = "Runs the fake nix checks task in the parallel workflow runner to verify nested flake checks are skipped.";
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
      parallel = true;
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
pkgs.runCommand "nix-checks-parent-workflow-skip-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  direct_log="$TMPDIR/direct-fake-nix.log"
  workflow_log="$TMPDIR/workflow-fake-nix.log"

  (
    unset NIX_BUILD_TOP
    FAKE_NIX_LOG="$direct_log" "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/direct.out" 2>&1
  )
  require_contains "$TMPDIR/direct.out" "INFO: checking flake checks ref=path:."
  require_contains "$TMPDIR/direct.out" "OK: flake checks passed ref=path:."
  require_contains "$direct_log" "flake show --no-write-lock-file path:."
  require_contains "$direct_log" "run path:.#help"
  require_contains "$direct_log" "flake check -L --no-write-lock-file path:."

  (
    unset NIX_BUILD_TOP
    FAKE_NIX_LOG="$workflow_log" "$EXECUTOR" run-workflow "${probeWorkflowId}" > "$TMPDIR/workflow.out" 2>&1
  )
  require_contains "$TMPDIR/workflow.out" "SKIP: flake checks skipped inside nix build sandbox or parent workflow ref=path:."
  require_contains "$workflow_log" "flake show --no-write-lock-file path:."
  require_contains "$workflow_log" "run path:.#help"
  require_not_contains "$workflow_log" "flake check --no-write-lock-file path:."

  echo "OK: parent workflow context skips nested flake checks" > "$out"
''
