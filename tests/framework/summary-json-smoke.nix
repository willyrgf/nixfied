{
  pkgs,
  model,
  registry,
}:
let
  orchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      model
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "summary-json-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"

  "$ORCH" run-workflow workflow.ci.basic --summary > "$TMPDIR/ci.out" 2>&1

  summary_file="$CI_ARTIFACTS_DIR/summary.json"
  if [ ! -f "$summary_file" ]; then
    echo "missing summary file: $summary_file"
    cat "$TMPDIR/ci.out"
    exit 1
  fi

  ${pkgs.jq}/bin/jq -e '
    .run_id
    and .workflow_id == "workflow.ci.basic"
    and .mode == "ci"
    and (.exit_code | type == "number")
    and (.started_at | type == "string")
    and (.finished_at | type == "string")
    and (.duration_seconds | type == "number")
    and (.counts.passed | type == "number")
    and (.counts.failed | type == "number")
    and (.counts.canceled | type == "number")
  ' "$summary_file" > /dev/null

  echo "OK: workflow summary json contract validated" > "$out"
''
