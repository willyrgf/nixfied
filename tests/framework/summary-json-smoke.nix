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
    and (.steps | type == "array")
    and (.steps | length >= 2)
    and ([.steps[] | .name | type] | all(. == "string"))
    and ([.steps[] | .status | type] | all(. == "string"))
    and ([.steps[] | .duration | type] | all(. == "number"))
    and (.timing.total_duration | type == "number")
    and (.timing.setup_duration | type == "number")
    and (.timing.steps_duration | type == "number")
    and (.timing.teardown_duration | type == "number")
    and (.timing.accounted_duration | type == "number")
    and (.timing.untracked_duration | type == "number")
    and ((.timing.parallelism.max_workers | type) == "number" or (.timing.parallelism.max_workers | type) == "null")
    and ((.timing.parallelism.peak_workers | type) == "number" or (.timing.parallelism.peak_workers | type) == "null")
    and ((.timing.parallelism.canceled_count | type) == "number" or (.timing.parallelism.canceled_count | type) == "null")
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
  json_payload="$(${pkgs.gawk}/bin/awk 'NF { line = $0 } END { print line }' "$TMPDIR/ci.json.out")"
  if [ -z "$json_payload" ]; then
    echo "missing json payload"
    cat "$TMPDIR/ci.json.out"
    exit 1
  fi
  printf '%s\n' "$json_payload" > "$TMPDIR/ci.json.payload"

  ${pkgs.jq}/bin/jq -e --arg runId "$json_run_id" '
    .run_id == $runId
    and .workflow_id == "workflow.ci.basic"
    and (.exit_code | type == "number")
    and (.summary_json | type == "string")
    and .summary.run_id == $runId
    and .summary.workflow_id == "workflow.ci.basic"
  ' "$TMPDIR/ci.json.payload" > /dev/null

  echo "OK: workflow summary json contract validated" > "$out"
''
