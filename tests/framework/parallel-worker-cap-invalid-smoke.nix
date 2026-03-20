{
  pkgs,
  model,
  services,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "parallel-worker-cap-invalid-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export NIXFIED_WORKFLOW_PARALLEL=1
  export NIXFIED_PARALLEL_SMOKE=1
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"

  set +e
  CI_MAX_WORKERS=0 "$EXECUTOR" run-workflow workflow.test.parallel.smoke --summary > "$TMPDIR/ci-max-workers-invalid.out" 2>&1
  bad_ci_rc="$?"
  set -e
  if [ "$bad_ci_rc" -eq 0 ]; then
    fail "expected CI_MAX_WORKERS=0 to fail"
  fi
  require_contains "$TMPDIR/ci-max-workers-invalid.out" "ERROR: CI_MAX_WORKERS must be an integer >= 1"

  set +e
  NIXFIED_CI_MAX_WORKERS=oops "$EXECUTOR" run-workflow workflow.test.parallel.smoke --summary > "$TMPDIR/nixfied-ci-max-workers-invalid.out" 2>&1
  bad_nixfied_rc="$?"
  set -e
  if [ "$bad_nixfied_rc" -eq 0 ]; then
    fail "expected NIXFIED_CI_MAX_WORKERS=oops to fail"
  fi
  require_contains "$TMPDIR/nixfied-ci-max-workers-invalid.out" "ERROR: NIXFIED_CI_MAX_WORKERS must be an integer >= 1"

  echo "OK: invalid parallel worker cap overrides fail fast" > "$out"
''
