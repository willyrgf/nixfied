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
pkgs.runCommand "logging-injection-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  "$ORCH" run-workflow workflow.ci.basic --run-id-file "$TMPDIR/workflow-logging.run-id" --summary --log-level info --output-mode stdout > "$TMPDIR/workflow-logging.out" 2>&1
  require_non_empty "$(read_trimmed_file "$TMPDIR/workflow-logging.run-id")" "workflow logging run id"

  set +e
  LOG_LEVEL=info NIXFIED_LOG_LEVEL=debug "$ORCH" run-task task.ci --mode basic --summary > "$TMPDIR/log-level-conflict.out" 2>&1
  conflict_rc="$?"
  set -e
  if [ "$conflict_rc" -eq 0 ]; then
    fail "expected conflicting LOG_LEVEL aliases to fail"
  fi
  require_contains "$TMPDIR/log-level-conflict.out" "ERROR: invocation LOG_LEVEL and NIXFIED_LOG_LEVEL conflict"

  set +e
  OUTPUT_MODE=stdout NIXFIED_OUTPUT_MODE=logs "$ORCH" run-task task.ci --mode basic --summary > "$TMPDIR/output-mode-conflict.out" 2>&1
  conflict_output_rc="$?"
  set -e
  if [ "$conflict_output_rc" -eq 0 ]; then
    fail "expected conflicting OUTPUT_MODE aliases to fail"
  fi
  require_contains "$TMPDIR/output-mode-conflict.out" "ERROR: invocation OUTPUT_MODE and NIXFIED_OUTPUT_MODE conflict"

  echo "OK: logging injection and alias conflicts are enforced" > "$out"
''
