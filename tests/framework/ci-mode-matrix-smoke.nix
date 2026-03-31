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
    disableEphemeralWorkflows = [
      "workflow.ci.basic"
      "workflow.ci.app"
      "workflow.ci.env"
      "workflow.ci.full"
    ];
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit
      services
      serviceDefinitions
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ci-mode-matrix-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  run_case() {
    local label="$1"
    local expected_workflow="$2"
    shift 2

    local log_file="$TMPDIR/$label.out"
    local run_id_file="$TMPDIR/$label.run-id"
    local summary_file="$TMPDIR/$label.summary.json"

    "$ORCH" run-task task.ci "$@" --run-id-file "$run_id_file" --summary-file "$summary_file" --summary > "$log_file" 2>&1

    local run_id
    run_id="$(read_trimmed_file "$run_id_file")"
    require_non_empty "$run_id" "run id for $label"
    require_file "$summary_file"

    ${pkgs.jq}/bin/jq -e --arg workflow "$expected_workflow" '
      .kind == "workflow-summary"
      and .version == 1
      and .payload.workflow_id == $workflow
      and .payload.mode == "ci"
      and .payload.exit_code == 0
      and (.payload.counts.passed | type == "number")
      and (.payload.counts.failed | type == "number")
      and (.payload.counts.canceled | type == "number")
      and (.payload.steps | type == "array")
      and (.payload.steps | length >= 2)
      and ([.payload.steps[] | .name | type] | all(. == "string"))
      and ([.payload.steps[] | .status | type] | all(. == "string"))
      and ([.payload.steps[] | .state | type] | all(. == "string"))
      and ([.payload.steps[] | (.workflow_id == null or (.workflow_id | type == "string"))] | all)
    ' "$summary_file" > /dev/null

    ${pkgs.jq}/bin/jq -e --arg runId "$run_id" --arg workflow "$expected_workflow" '
      select((.payload.runId // "") == $runId and (.payload.workflowId // "") == $workflow) | .payload.runId
    ' "$REGISTRY_ROOT/events.ndjson" > /dev/null
  }

  run_case "mode-basic" "workflow.ci.basic" --basic
  run_case "mode-app" "workflow.ci.app" --app
  run_case "mode-env" "workflow.ci.env" --env
  run_case "mode-full" "workflow.ci.full" --full
  run_case "mode-option" "workflow.ci.app" --mode app
  run_case "mode-option-logging" "workflow.ci.app" --mode app --log-level debug --output-mode both

  set +e
  "$ORCH" run-task task.ci --mode nope --summary > "$TMPDIR/mode-invalid.out" 2>&1
  invalid_rc="$?"
  set -e
  if [ "$invalid_rc" -eq 0 ]; then
    fail "expected --mode nope to fail"
  fi
  require_contains "$TMPDIR/mode-invalid.out" "ERROR: unknown mode 'nope' (expected:"

  set +e
  "$ORCH" run-task task.ci --mode basic --summary --log-level nope > "$TMPDIR/log-level-invalid.out" 2>&1
  invalid_log_level_rc="$?"
  set -e
  if [ "$invalid_log_level_rc" -eq 0 ]; then
    fail "expected --log-level nope to fail"
  fi
  require_contains "$TMPDIR/log-level-invalid.out" "ERROR: invalid --log-level 'nope'"

  set +e
  "$ORCH" run-task task.ci --mode basic --summary --output-mode nope > "$TMPDIR/output-mode-invalid.out" 2>&1
  invalid_output_mode_rc="$?"
  set -e
  if [ "$invalid_output_mode_rc" -eq 0 ]; then
    fail "expected --output-mode nope to fail"
  fi
  require_contains "$TMPDIR/output-mode-invalid.out" "ERROR: invalid --output-mode 'nope'"

  echo "OK: ci mode matrix aliases resolve deterministic workflows" > "$out"
''
