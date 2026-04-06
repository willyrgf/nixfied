{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  probeModel = import ./lib/ci-probe-model.nix {
    inherit
      pkgs
      model
      ;
    disableEphemeralWorkflows = [ "workflow.ci.basic" ];
  };
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit
      pkgs
      services
      serviceDefinitions
      ;
    model = probeModel;
  };

  orchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit services;
    projectRoot = ../..;
    serviceDispatcherProgram = runtimeDeps.serviceDispatcherProgram;
    runtimeBin = runtimeDeps.serviceDispatcherProgram;
  };
in
pkgs.runCommand "summary-json-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  JQ=${pkgs.jq}/bin/jq
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  run_id_file="$TMPDIR/ci.run-id"
  summary_file="$TMPDIR/ci.summary.json"
  BENIGN_AMBIENT_VAR=alpha \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$run_id_file" --summary-file "$summary_file" --summary > "$TMPDIR/ci.out" 2>&1

  run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$run_id_file")"
  if [ -z "$run_id" ]; then
    echo "missing run id"
    cat "$TMPDIR/ci.out"
    exit 1
  fi
  if [ ! -f "$summary_file" ]; then
    echo "missing summary file: $summary_file"
    cat "$TMPDIR/ci.out"
    exit 1
  fi
  attempt_id="$("$JQ" -r '.payload.attempt_id' "$summary_file")"
  if [ -z "$attempt_id" ] || [ "$attempt_id" = "null" ]; then
    echo "missing attempt id in first summary"
    cat "$summary_file"
    exit 1
  fi
  run_artifacts_summary="$(find "$CI_ARTIFACTS_ROOT" -type f -path "*/$run_id/$attempt_id/summary.json" | head -n 1 || true)"
  if [ -z "$run_artifacts_summary" ] || [ ! -f "$run_artifacts_summary" ]; then
    echo "missing attempt-scoped artifacts summary for first run"
    find "$CI_ARTIFACTS_ROOT" -type f | sort
    exit 1
  fi

  "$JQ" -e '
    .kind == "workflow-summary"
    and .version == 1
    and .payload.run_id
    and (.payload.attempt_id | type == "string")
    and .payload.workflow_id == "workflow.ci.basic"
    and .payload.mode == "ci"
    and (.payload.exit_code | type == "number")
    and (.payload.started_at | type == "string")
    and (.payload.finished_at | type == "string")
    and (.payload.duration_seconds | type == "number")
    and (.payload.counts.passed | type == "number")
    and (.payload.counts.failed | type == "number")
    and (.payload.counts.canceled | type == "number")
    and (.payload.steps | type == "array")
    and (.payload.steps | length >= 2)
    and ([.payload.steps[] | .name | type] | all(. == "string"))
    and ([.payload.steps[] | .status | type] | all(. == "string"))
    and ([.payload.steps[] | .duration | type] | all(. == "number"))
    and (.payload.timing.total_duration | type == "number")
    and (.payload.timing.setup_duration | type == "number")
    and (.payload.timing.steps_duration | type == "number")
    and (.payload.timing.teardown_duration | type == "number")
    and (.payload.timing.accounted_duration | type == "number")
    and (.payload.timing.untracked_duration | type == "number")
    and ((.payload.timing.parallelism.max_workers | type) == "number" or (.payload.timing.parallelism.max_workers | type) == "null")
    and ((.payload.timing.parallelism.peak_workers | type) == "number" or (.payload.timing.parallelism.peak_workers | type) == "null")
    and ((.payload.timing.parallelism.canceled_count | type) == "number" or (.payload.timing.parallelism.canceled_count | type) == "null")
  ' "$summary_file" > /dev/null

  ${pkgs.gnugrep}/bin/grep -Fq "Summary" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "[PASS] task.ci.quality" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "[PASS] task.ci.tests" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "Total time:" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Time breakdown" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Parallelism" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: Exit code: 0" "$TMPDIR/ci.out"

  events_file="$REGISTRY_ROOT/events.ndjson"
  if [ ! -s "$events_file" ]; then
    echo "missing registry events file"
    exit 1
  fi
  "$JQ" -s -e 'length > 0 and all(.[]; .kind == "runtime-event" and .version == 1)' "$events_file" > /dev/null

  rerun_id_file="$TMPDIR/ci-rerun.run-id"
  rerun_summary_file="$TMPDIR/ci-rerun.summary.json"
  BENIGN_AMBIENT_VAR=beta \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$rerun_id_file" --summary-file "$rerun_summary_file" > "$TMPDIR/ci.rerun.out" 2>&1

  rerun_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$rerun_id_file")"
  if [ -z "$rerun_id" ]; then
    echo "missing rerun id"
    cat "$TMPDIR/ci.rerun.out"
    exit 1
  fi
  if [ "$run_id" != "$rerun_id" ]; then
    echo "run ids differ for equivalent workflow invocations"
    echo "run_id=$run_id"
    echo "rerun_id=$rerun_id"
    exit 1
  fi
  rerun_attempt_id="$("$JQ" -r '.payload.attempt_id' "$rerun_summary_file")"
  if [ -z "$rerun_attempt_id" ] || [ "$rerun_attempt_id" = "null" ]; then
    echo "missing attempt id in second summary"
    cat "$rerun_summary_file"
    exit 1
  fi
  if [ "$attempt_id" = "$rerun_attempt_id" ]; then
    echo "attempt ids should differ across repeated equivalent invocations"
    echo "attempt_id=$attempt_id"
    echo "rerun_attempt_id=$rerun_attempt_id"
    exit 1
  fi
  rerun_artifacts_summary="$(find "$CI_ARTIFACTS_ROOT" -type f -path "*/$rerun_id/$rerun_attempt_id/summary.json" | head -n 1 || true)"
  if [ -z "$rerun_artifacts_summary" ] || [ ! -f "$rerun_artifacts_summary" ]; then
    echo "missing attempt-scoped artifacts summary for second run"
    find "$CI_ARTIFACTS_ROOT" -type f | sort
    exit 1
  fi
  if [ "$run_artifacts_summary" = "$rerun_artifacts_summary" ]; then
    echo "equivalent reruns should not reuse the same attempt artifacts summary path"
    echo "run_artifacts_summary=$run_artifacts_summary"
    echo "rerun_artifacts_summary=$rerun_artifacts_summary"
    exit 1
  fi
  first_step_count="$("$JQ" -r '.payload.steps | length' "$summary_file")"
  second_step_count="$("$JQ" -r '.payload.steps | length' "$rerun_summary_file")"
  if [ "$first_step_count" != "$second_step_count" ]; then
    echo "equivalent rerun should not duplicate summary steps"
    echo "first_step_count=$first_step_count"
    echo "second_step_count=$second_step_count"
    exit 1
  fi

  if BENIGN_AMBIENT_VAR=gamma \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$TMPDIR/rejected.run-id" --summary-file "$TMPDIR/rejected.summary.json" --json > "$TMPDIR/ci.json.rejected.out" 2>&1; then
    echo "workflow --json should be rejected"
    cat "$TMPDIR/ci.json.rejected.out"
    exit 1
  fi
  ${pkgs.gnugrep}/bin/grep -Fq "ERROR: unknown option '--json'" "$TMPDIR/ci.json.rejected.out"

  echo "OK: workflow summary sidecars stay stable and workflow stdout json is removed" > "$out"
''
