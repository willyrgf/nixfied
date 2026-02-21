{
  pkgs,
  registry,
}:
let
  workflowId = "workflow.test.parallel.smoke";

  frameworkLib = import ../../nixfied/lib {
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

  "$ORCH" run-workflow ${workflowId} --summary > "$TMPDIR/run-1.out" 2>&1
  "$ORCH" run-workflow ${workflowId} --summary > "$TMPDIR/run-2.out" 2>&1

  run_1="$(extract_run_id "$TMPDIR/run-1.out")"
  run_2="$(extract_run_id "$TMPDIR/run-2.out")"
  summary_1="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$TMPDIR/run-1.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  summary_2="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$TMPDIR/run-2.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$run_1" "run_1"
  require_non_empty "$run_2" "run_2"
  require_non_empty "$summary_1" "summary_1"
  require_non_empty "$summary_2" "summary_2"

  if [ "$run_1" = "$run_2" ]; then
    fail "expected unique run IDs for repeated workflow runs"
  fi

  require_file "$summary_1"
  require_file "$summary_2"

  case "$summary_1" in
    */"$run_1"/summary.json) ;;
    *)
      fail "summary path is not run-scoped for run_1: $summary_1"
      ;;
  esac

  case "$summary_2" in
    */"$run_2"/summary.json) ;;
    *)
      fail "summary path is not run-scoped for run_2: $summary_2"
      ;;
  esac

  ${pkgs.jq}/bin/jq -e --arg run "$run_1" '.run_id == $run' "$summary_1" > /dev/null
  ${pkgs.jq}/bin/jq -e --arg run "$run_2" '.run_id == $run' "$summary_2" > /dev/null

  echo "OK: workflow artifacts are isolated per run id" > "$out"
''
