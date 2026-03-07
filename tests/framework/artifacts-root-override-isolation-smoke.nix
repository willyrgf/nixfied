{
  pkgs,
  model,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "artifacts-root-override-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  set +e
  CI_ARTIFACTS_ROOT="$TMPDIR/root-override" "$ORCH" run-workflow workflow.ci.basic --summary > "$TMPDIR/root-1.out" 2>&1 &
  pid_one="$!"
  CI_ARTIFACTS_ROOT="$TMPDIR/root-override" "$ORCH" run-workflow workflow.ci.basic --summary > "$TMPDIR/root-2.out" 2>&1 &
  pid_two="$!"
  wait "$pid_one"
  rc_one="$?"
  wait "$pid_two"
  rc_two="$?"
  set -e

  if [ "$rc_one" -ne 0 ] || [ "$rc_two" -ne 0 ]; then
    fail "expected root override workflow runs to pass"
  fi

  run_one="$(extract_run_id "$TMPDIR/root-1.out")"
  run_two="$(extract_run_id "$TMPDIR/root-2.out")"
  summary_one="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$TMPDIR/root-1.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  summary_two="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$TMPDIR/root-2.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$run_one" "run_one"
  require_non_empty "$run_two" "run_two"
  require_non_empty "$summary_one" "summary_one"
  require_non_empty "$summary_two" "summary_two"
  require_file "$summary_one"
  require_file "$summary_two"

  case "$summary_one" in
    "$TMPDIR/root-override/$run_one/summary.json") ;;
    *)
      fail "unexpected root override summary path: $summary_one"
      ;;
  esac

  case "$summary_two" in
    "$TMPDIR/root-override/$run_two/summary.json") ;;
    *)
      fail "unexpected root override summary path: $summary_two"
      ;;
  esac

  set +e
  CI_ARTIFACTS_DIR="$TMPDIR/flat-override" "$ORCH" run-workflow workflow.ci.basic --summary > "$TMPDIR/flat.out" 2>&1
  flat_rc="$?"
  set -e
  if [ "$flat_rc" -ne 0 ]; then
    fail "expected flat artifacts dir override to be normalized, not rejected"
  fi

  flat_run="$(extract_run_id "$TMPDIR/flat.out")"
  flat_summary="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$TMPDIR/flat.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$flat_run" "flat_run"
  require_non_empty "$flat_summary" "flat_summary"
  require_file "$flat_summary"

  case "$flat_summary" in
    "$TMPDIR/flat-override/$flat_run/summary.json") ;;
    *)
      fail "expected CI_ARTIFACTS_DIR override to be normalized per run: $flat_summary"
      ;;
  esac

  echo "OK: artifacts overrides remain run-scoped" > "$out"
''
