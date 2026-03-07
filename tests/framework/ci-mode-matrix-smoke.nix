{
  pkgs,
  model,
  registry,
}:
let
  probeModel = model // {
    workflows = model.workflows // {
      "workflow.ci.basic" = model.workflows."workflow.ci.basic" // {
        execution = model.workflows."workflow.ci.basic".execution // {
          ephemeral = (model.workflows."workflow.ci.basic".execution.ephemeral or { }) // {
            enable = false;
          };
        };
      };
      "workflow.ci.app" = model.workflows."workflow.ci.app" // {
        execution = model.workflows."workflow.ci.app".execution // {
          ephemeral = (model.workflows."workflow.ci.app".execution.ephemeral or { }) // {
            enable = false;
          };
        };
      };
      "workflow.ci.env" = model.workflows."workflow.ci.env" // {
        execution = model.workflows."workflow.ci.env".execution // {
          ephemeral = (model.workflows."workflow.ci.env".execution.ephemeral or { }) // {
            enable = false;
          };
        };
      };
      "workflow.ci.full" = model.workflows."workflow.ci.full" // {
        execution = model.workflows."workflow.ci.full".execution // {
          ephemeral = (model.workflows."workflow.ci.full".execution.ephemeral or { }) // {
            enable = false;
          };
        };
      };
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ci-mode-matrix-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  run_case() {
    local label="$1"
    local expected_workflow="$2"
    shift 2

    local log_file="$TMPDIR/$label.out"
    local artifacts_dir="$TMPDIR/artifacts-$label"
    mkdir -p "$artifacts_dir"

    CI_ARTIFACTS_DIR="$artifacts_dir" "$ORCH" run-task task.ci "$@" --summary > "$log_file" 2>&1

    local run_id
    local summary_file
    run_id="$(extract_run_id "$log_file")"
    require_non_empty "$run_id" "run id for $label"
    summary_file="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$log_file" | ${pkgs.coreutils}/bin/tail -n 1)"
    require_non_empty "$summary_file" "summary file for $label"

    require_file "$summary_file"
    case "$summary_file" in
      "$artifacts_dir/$run_id/summary.json") ;;
      *)
        fail "expected run-scoped summary path for $label: $summary_file"
        ;;
    esac

    ${pkgs.jq}/bin/jq -e --arg workflow "$expected_workflow" '
      .workflow_id == $workflow
      and .mode == "ci"
      and .exit_code == 0
      and (.counts.passed | type == "number")
      and (.counts.failed | type == "number")
      and (.counts.canceled | type == "number")
    ' "$summary_file" > /dev/null

    ${pkgs.jq}/bin/jq -e --arg runId "$run_id" --arg workflow "$expected_workflow" '
      select(.runId == $runId and .workflowId == $workflow) | .runId
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
