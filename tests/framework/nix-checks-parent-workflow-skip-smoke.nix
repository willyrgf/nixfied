{
  pkgs,
  model,
  services,
  serviceDefinitions,
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

  fakeDeadnix = pkgs.writeShellScriptBin "deadnix" ''
    set -euo pipefail

    if [ -z "''${FAKE_DEADNIX_LOG:-}" ]; then
      echo "FAKE_DEADNIX_LOG is required" >&2
      exit 1
    fi

    if [ "$#" -eq 1 ]; then
      printf '%s\n' "$1" >> "$FAKE_DEADNIX_LOG"
    fi

    if [ "$#" -lt 1 ]; then
      echo "unexpected deadnix args: $*" >&2
      exit 1
    fi
  '';

  fakeStatix = pkgs.writeShellScriptBin "statix" ''
    set -euo pipefail

    if [ -z "''${FAKE_STATIX_LOG:-}" ]; then
      echo "FAKE_STATIX_LOG is required" >&2
      exit 1
    fi

    if [ "$#" -eq 2 ]; then
      printf '%s\n' "$2" >> "$FAKE_STATIX_LOG"
    fi

    if [ "$#" -lt 2 ] || [ "$1" != "check" ]; then
      echo "unexpected statix args: $*" >&2
      exit 1
    fi
  '';

  fakeNil = pkgs.writeShellScriptBin "nil" ''
    set -euo pipefail

    if [ -z "''${FAKE_NIL_LOG:-}" ]; then
      echo "FAKE_NIL_LOG is required" >&2
      exit 1
    fi

    if [ "$#" -eq 2 ]; then
      printf '%s\n' "$2" >> "$FAKE_NIL_LOG"
    fi

    if [ "$#" -lt 2 ] || [ "$1" != "diagnostics" ]; then
      echo "unexpected nil args: $*" >&2
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
    deadnix = fakeDeadnix;
    nix = fakeNix;
    nil = fakeNil;
    statix = fakeStatix;
  };

  fakeNixChecksPkg =
    import ../../nixfied/framework/core/mkNixChecks.nix
      {
        pkgs = fakePkgs;
        inherit (pkgs) lib;
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
      passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [
        "FAKE_DEADNIX_LOG"
        "FAKE_NIX_LOG"
        "FAKE_NIL_LOG"
        "FAKE_STATIX_LOG"
      ];
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
      root = "artifacts-root";
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
    inherit
      services
      serviceDefinitions
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "nix-checks-parent-workflow-skip-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  direct_log="$TMPDIR/direct-fake-nix.log"
  direct_deadnix_log="$TMPDIR/direct-fake-deadnix.log"
  workflow_log="$TMPDIR/workflow-fake-nix.log"
  workflow_deadnix_log="$TMPDIR/workflow-fake-deadnix.log"
  direct_nil_log="$TMPDIR/direct-fake-nil.log"
  workflow_nil_log="$TMPDIR/workflow-fake-nil.log"
  direct_statix_log="$TMPDIR/direct-fake-statix.log"
  workflow_statix_log="$TMPDIR/workflow-fake-statix.log"

  (
    unset NIX_BUILD_TOP
    FAKE_DEADNIX_LOG="$direct_deadnix_log" FAKE_NIX_LOG="$direct_log" FAKE_NIL_LOG="$direct_nil_log" FAKE_STATIX_LOG="$direct_statix_log" \
      "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/direct.out" 2>&1
  )
  require_contains "$TMPDIR/direct.out" "INFO: checking dead"
  require_contains "$TMPDIR/direct.out" "OK: dead"
  require_contains "$TMPDIR/direct.out" "INFO: checking statix"
  require_contains "$TMPDIR/direct.out" "OK: statix"
  require_contains "$TMPDIR/direct.out" "INFO: checking nil diagnostics files="
  require_contains "$TMPDIR/direct.out" "OK: nil diagnostics check passed files="
  require_contains "$TMPDIR/direct.out" "INFO: checking flake output surface ref="
  require_contains "$TMPDIR/direct.out" "OK: flake output surface check passed ref="
  if /nix/store/k2y0040hinbk3a2jj5ppy2dp2bpy9nw3-gnugrep-3.12/bin/grep -Fq "SKIP: flake checks skipped inside nix build sandbox or parent workflow ref=" "$TMPDIR/direct.out"; then
    require_not_contains "$direct_log" "flake check -L --no-write-lock-file"
  else
    require_contains "$TMPDIR/direct.out" "OK: flake checks passed ref="
    require_contains "$direct_log" "flake check -L --no-write-lock-file"
  fi
  require_contains "$TMPDIR/direct.out" "INFO: checking flake checks ref="
  if [ ! -s "$direct_deadnix_log" ]; then
    fail "expected non-empty $direct_deadnix_log"
  fi
  if [ ! -s "$direct_nil_log" ]; then
    fail "expected non-empty $direct_nil_log"
  fi
  if [ ! -s "$direct_statix_log" ]; then
    fail "expected non-empty $direct_statix_log"
  fi
  require_contains "$direct_log" "flake show --no-write-lock-file"

  (
    unset NIX_BUILD_TOP
    FAKE_DEADNIX_LOG="$workflow_deadnix_log" FAKE_NIX_LOG="$workflow_log" FAKE_NIL_LOG="$workflow_nil_log" FAKE_STATIX_LOG="$workflow_statix_log" \
      "$EXECUTOR" run-workflow "${probeWorkflowId}" > "$TMPDIR/workflow.out" 2>&1
  )
  require_contains "$TMPDIR/workflow.out" "INFO: checking dead"
  require_contains "$TMPDIR/workflow.out" "OK: dead"
  require_contains "$TMPDIR/workflow.out" "INFO: checking statix"
  require_contains "$TMPDIR/workflow.out" "OK: statix"
  require_contains "$TMPDIR/workflow.out" "INFO: checking nil diagnostics files="
  require_contains "$TMPDIR/workflow.out" "OK: nil diagnostics check passed files="
  require_contains "$TMPDIR/workflow.out" "SKIP: flake checks skipped inside nix build sandbox or parent workflow ref="
  if [ ! -s "$workflow_deadnix_log" ]; then
    fail "expected non-empty $workflow_deadnix_log"
  fi
  if [ ! -s "$workflow_nil_log" ]; then
    fail "expected non-empty $workflow_nil_log"
  fi
  if [ ! -s "$workflow_statix_log" ]; then
    fail "expected non-empty $workflow_statix_log"
  fi
  require_contains "$workflow_log" "flake show --no-write-lock-file"
  require_not_contains "$workflow_log" "flake check -L --no-write-lock-file"

  echo "OK: parent workflow context skips nested flake checks" > "$out"
''
