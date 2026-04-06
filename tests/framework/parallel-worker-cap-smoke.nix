{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit
      model
      services
      serviceDefinitions
      ;
  };
  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
    inherit (runtimeDeps) serviceDispatcherProgram;
    runtimeBin = serviceDispatcherProgram;
  };
in
pkgs.runCommand "parallel-worker-cap-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  export NIXFIED_WORKFLOW_PARALLEL=1
  export NIXFIED_PARALLEL_SMOKE=1
  export NIXFIED_CI_MAX_WORKERS=1
  export CI_MAX_WORKERS=1
  unset NIXFIED_WORKFLOW_NESTED
  unset NIXFIED_ORCHESTRATOR_RUN_ID

  run_id_file="$TMPDIR/out.run-id"
  "$EXECUTOR" run-workflow workflow.test.parallel.smoke --run-id-file "$run_id_file" --summary > "$TMPDIR/out.log" 2>&1
  run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$run_id_file")"
  if [ -z "$run_id" ] && [ -f "$REGISTRY_ROOT/events.ndjson" ]; then
    run_id="$(${pkgs.jq}/bin/jq -r 'select((.payload.workflowId // "") == "workflow.test.parallel.smoke" and (.payload.taskId // "") == "") | .payload.runId' "$REGISTRY_ROOT/events.ndjson" | ${pkgs.coreutils}/bin/tail -n 1)"
  fi
  if [ -z "$run_id" ]; then
    echo "missing run id"
    cat "$TMPDIR/out.log"
    if [ -f "$REGISTRY_ROOT/events.ndjson" ]; then
      cat "$REGISTRY_ROOT/events.ndjson"
    fi
    exit 1
  fi

  max_running="$(${pkgs.jq}/bin/jq -s -r --arg runId "$run_id" '
    map(select((.payload.runId // "") == $runId and (.payload.taskId // "") != ""))
    | sort_by((.payload.seq // 0))
    | reduce .[] as $event ({running: 0, max: 0};
      if ($event.payload.state // "") == "running" then
        .running += 1 |
        .max = (if .running > .max then .running else .max end)
      elif (($event.payload.state // "") == "passed" or ($event.payload.state // "") == "failed" or ($event.payload.state // "") == "canceled") then
        .running = (if .running > 0 then .running - 1 else 0 end)
      else
        .
      end
    ) | .max
  ' "$REGISTRY_ROOT/events.ndjson")"

  if ! [[ "$max_running" =~ ^[0-9]+$ ]]; then
    echo "expected integer max running count, got '$max_running'"
    cat "$TMPDIR/out.log"
    cat "$REGISTRY_ROOT/events.ndjson"
    exit 1
  fi

  if [ "$max_running" -gt 1 ]; then
    echo "expected max running tasks <= 1 with CI_MAX_WORKERS=1, got $max_running"
    cat "$TMPDIR/out.log"
    cat "$REGISTRY_ROOT/events.ndjson"
    exit 1
  fi

  echo "OK: parallel worker cap override is enforced" > "$out"
''
