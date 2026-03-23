{
  pkgs,
  registry,
}:
let
  workflowId = "workflow.test.parallel.smoke";

  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied.workflows.test-parallel-smoke.artifacts.root = lib.mkForce "artifacts-root";
        }
      )
    ];
    localOverrides = [ ];
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    services = compiled.services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "artifacts-run-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"
  unset CI_ARTIFACTS_DIR || true

  set +e
  "$ORCH" run-workflow ${workflowId} --run-id-file "$TMPDIR/run-1.run-id" --summary > "$TMPDIR/run-1.out" 2>&1 &
  pid_one="$!"
  "$ORCH" run-workflow ${workflowId} --run-id-file "$TMPDIR/run-2.run-id" --summary > "$TMPDIR/run-2.out" 2>&1 &
  pid_two="$!"
  wait "$pid_one"
  rc_one="$?"
  wait "$pid_two"
  rc_two="$?"
  set -e

  if [ "$rc_one" -ne 0 ] || [ "$rc_two" -ne 0 ]; then
    fail "expected concurrent workflow runs to pass"
  fi

  run_1="$(read_trimmed_file "$TMPDIR/run-1.run-id")"
  run_2="$(read_trimmed_file "$TMPDIR/run-2.run-id")"
  summary_1="$(find artifacts-root -type f -path "*/$run_1/*/summary.json" | head -n 1 || true)"
  summary_2="$(find artifacts-root -type f -path "*/$run_2/*/summary.json" | head -n 1 || true)"
  attempt_1=""
  attempt_2=""
  require_non_empty "$run_1" "run_1"
  require_non_empty "$run_2" "run_2"
  require_non_empty "$summary_1" "summary_1"
  require_non_empty "$summary_2" "summary_2"

  if [ "$run_1" = "$run_2" ]; then
    fail "expected unique run IDs for repeated workflow runs"
  fi

  require_file "$summary_1"
  require_file "$summary_2"
  attempt_1="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$summary_1")"
  attempt_2="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$summary_2")"
  require_non_empty "$attempt_1" "attempt_1"
  require_non_empty "$attempt_2" "attempt_2"

  case "$summary_1" in
    */"$run_1"/"$attempt_1"/summary.json) ;;
    *)
      fail "summary path is not run+attempt scoped for run_1: $summary_1"
      ;;
  esac

  case "$summary_2" in
    */"$run_2"/"$attempt_2"/summary.json) ;;
    *)
      fail "summary path is not run+attempt scoped for run_2: $summary_2"
      ;;
  esac

  ${pkgs.jq}/bin/jq -e --arg run "$run_1" '.payload.run_id == $run' "$summary_1" > /dev/null
  ${pkgs.jq}/bin/jq -e --arg run "$run_2" '.payload.run_id == $run' "$summary_2" > /dev/null

  echo "OK: workflow artifacts are isolated per run and attempt" > "$out"
''
