{
  pkgs,
  model,
  registry,
}:
let
  probeModel = import ./lib/ci-probe-model.nix {
    inherit
      pkgs
      model
      ;
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
pkgs.runCommand "artifacts-root-override-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked artifacts root probe\n' > "$repo/tracked.txt"
  printf 'tracked subdir probe\n' > "$repo/subdir/probe.txt"
  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/probe.txt
  ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -m "init artifacts root override probe repo" >/dev/null 2>&1

  set +e
  CI_ARTIFACTS_ROOT="$TMPDIR/root-override" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$TMPDIR/root-1.run-id" --summary > "$TMPDIR/root-1.out" 2>&1 &
  pid_one="$!"
  CI_ARTIFACTS_ROOT="$TMPDIR/root-override" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$TMPDIR/root-2.run-id" --summary > "$TMPDIR/root-2.out" 2>&1 &
  pid_two="$!"
  wait "$pid_one"
  rc_one="$?"
  wait "$pid_two"
  rc_two="$?"
  set -e

  if [ "$rc_one" -ne 0 ] || [ "$rc_two" -ne 0 ]; then
    fail "expected root override workflow runs to pass"
  fi

  run_one="$(read_trimmed_file "$TMPDIR/root-1.run-id")"
  run_two="$(read_trimmed_file "$TMPDIR/root-2.run-id")"
  summary_one="$TMPDIR/root-override/$run_one/summary.json"
  summary_two="$TMPDIR/root-override/$run_two/summary.json"
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
  CI_ARTIFACTS_DIR="$TMPDIR/flat-override" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$TMPDIR/flat.run-id" --summary > "$TMPDIR/flat.out" 2>&1
  flat_rc="$?"
  set -e
  if [ "$flat_rc" -ne 0 ]; then
    fail "expected flat artifacts dir override to be normalized, not rejected"
  fi

  flat_run="$(read_trimmed_file "$TMPDIR/flat.run-id")"
  flat_summary="$TMPDIR/flat-override/$flat_run/summary.json"
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
