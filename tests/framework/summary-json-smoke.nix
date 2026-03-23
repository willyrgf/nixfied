{
  pkgs,
  model,
  services,
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

  orchestrator = import ../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "summary-json-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
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
  attempt_id="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$summary_file")"
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

  ${pkgs.jq}/bin/jq -e '
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

  json_run_id_file="$TMPDIR/ci-json.run-id"
  json_summary_file="$TMPDIR/ci-json.summary.json"
  BENIGN_AMBIENT_VAR=beta \
    "$ORCH" run-workflow workflow.ci.basic --run-id-file "$json_run_id_file" --summary-file "$json_summary_file" --json > "$TMPDIR/ci.json.out" 2>&1

  json_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$json_run_id_file")"
  if [ -z "$json_run_id" ]; then
    echo "missing json run id"
    cat "$TMPDIR/ci.json.out"
    exit 1
  fi
  if [ "$run_id" != "$json_run_id" ]; then
    echo "run ids differ for equivalent workflow invocations"
    echo "run_id=$run_id"
    echo "json_run_id=$json_run_id"
    exit 1
  fi
  json_attempt_id="$(${pkgs.jq}/bin/jq -r '.payload.attempt_id' "$json_summary_file")"
  if [ -z "$json_attempt_id" ] || [ "$json_attempt_id" = "null" ]; then
    echo "missing attempt id in second summary"
    cat "$json_summary_file"
    exit 1
  fi
  if [ "$attempt_id" = "$json_attempt_id" ]; then
    echo "attempt ids should differ across repeated equivalent invocations"
    echo "attempt_id=$attempt_id"
    echo "json_attempt_id=$json_attempt_id"
    exit 1
  fi
  json_run_artifacts_summary="$(find "$CI_ARTIFACTS_ROOT" -type f -path "*/$json_run_id/$json_attempt_id/summary.json" | head -n 1 || true)"
  if [ -z "$json_run_artifacts_summary" ] || [ ! -f "$json_run_artifacts_summary" ]; then
    echo "missing attempt-scoped artifacts summary for second run"
    find "$CI_ARTIFACTS_ROOT" -type f | sort
    exit 1
  fi
  if [ "$run_artifacts_summary" = "$json_run_artifacts_summary" ]; then
    echo "equivalent reruns should not reuse the same attempt artifacts summary path"
    echo "run_artifacts_summary=$run_artifacts_summary"
    echo "json_run_artifacts_summary=$json_run_artifacts_summary"
    exit 1
  fi
  first_step_count="$(${pkgs.jq}/bin/jq -r '.payload.steps | length' "$summary_file")"
  second_step_count="$(${pkgs.jq}/bin/jq -r '.payload.steps | length' "$json_summary_file")"
  if [ "$first_step_count" != "$second_step_count" ]; then
    echo "equivalent rerun should not duplicate summary steps"
    echo "first_step_count=$first_step_count"
    echo "second_step_count=$second_step_count"
    exit 1
  fi
  json_payload="$(${pkgs.gawk}/bin/awk 'NF { line = $0 } END { print line }' "$TMPDIR/ci.json.out")"
  if [ -z "$json_payload" ]; then
    echo "missing json payload"
    cat "$TMPDIR/ci.json.out"
    exit 1
  fi
  printf '%s\n' "$json_payload" > "$TMPDIR/ci.json.payload"

  ${pkgs.jq}/bin/jq -e --arg runId "$json_run_id" '
    .run_id == $runId
    and (.attempt_id | type == "string")
    and .workflow_id == "workflow.ci.basic"
    and (.exit_code | type == "number")
    and (.summary_json | type == "string")
    and .summary.kind == "workflow-summary"
    and .summary.version == 1
    and .summary.payload.run_id == $runId
    and (.summary.payload.attempt_id | type == "string")
    and .summary.payload.workflow_id == "workflow.ci.basic"
  ' "$TMPDIR/ci.json.payload" > /dev/null

  echo "OK: workflow summary json contract validated" > "$out"
''
