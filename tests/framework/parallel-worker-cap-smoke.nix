{
  pkgs,
  model,
  registry,
}:
let
  executor = import ../../nixfied/runner/executor.nix {
    inherit
      pkgs
      model
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "parallel-worker-cap-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  export NIXFIED_WORKFLOW_PARALLEL=1
  export NIXFIED_PARALLEL_SMOKE=1
  export CI_MAX_WORKERS=1

  "$EXECUTOR" run-workflow workflow.test.parallel.smoke --summary > "$TMPDIR/out.log" 2>&1
  run_id="$(${pkgs.gnused}/bin/sed -n 's/^INFO: runId=\([^ ]*\).*/\1/p' "$TMPDIR/out.log" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ -z "$run_id" ]; then
    echo "missing run id"
    cat "$TMPDIR/out.log"
    exit 1
  fi

  max_running="$(${pkgs.jq}/bin/jq -s -r --arg runId "$run_id" '
    map(select(.runId == $runId and (.taskId // "") != ""))
    | sort_by(.seq)
    | reduce .[] as $event ({running: 0, max: 0};
      if $event.state == "running" then
        .running += 1 |
        .max = (if .running > .max then .running else .max end)
      elif ($event.state == "passed" or $event.state == "failed" or $event.state == "canceled") then
        .running = (if .running > 0 then .running - 1 else 0 end)
      else
        .
      end
    ) | .max
  ' "$REGISTRY_ROOT/events.ndjson")"

  if [ "$max_running" -gt 1 ]; then
    echo "expected max running tasks <= 1 with CI_MAX_WORKERS=1, got $max_running"
    exit 1
  fi

  echo "OK: parallel worker cap override is enforced" > "$out"
''
