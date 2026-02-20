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
pkgs.runCommand "workflow-mode-derived-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"

  NIXFIED_PARALLEL_SMOKE=1 "$ORCH" run-workflow workflow.test.parallel.smoke --mode parallel.smoke --summary > "$TMPDIR/mode-complex.out" 2>&1
  require_contains "$TMPDIR/mode-complex.out" "INFO: runId="
  require_contains "$TMPDIR/mode-complex.out" "INFO: summary_json="

  set +e
  NIXFIED_PARALLEL_SMOKE=1 "$ORCH" run-workflow workflow.test.parallel.smoke --parallel-smoke --summary > "$TMPDIR/mode-shorthand-invalid.out" 2>&1
  invalid_rc="$?"
  set -e
  if [ "$invalid_rc" -eq 0 ]; then
    fail "expected dotted mode shorthand to fail"
  fi
  require_contains "$TMPDIR/mode-shorthand-invalid.out" "ERROR: unknown option '--parallel-smoke'"

  echo "OK: workflow mode resolution is model-driven across non-ci families" > "$out"
''
