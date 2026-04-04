{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  repoRoot = builtins.toString ../..;

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ../../nixfied/framework/testing/repo-overlay.nix ];
    localOverrides = [ ./lib/test-probe-overrides.nix ];
  };

  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
in
pkgs.runCommand "test-mode-cli-contract-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  TEST_APP="${frameworkOutputs.apps.test.program}"
  JQ="${pkgs.jq}/bin/jq"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export HOME="$TMPDIR/home"
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR" "$HOME"
  cd "$NIXFIED_FLAKE_ROOT"

  "$TEST_APP" --help > "$TMPDIR/help.out" 2>&1
  require_contains "$TMPDIR/help.out" "test - Run tests"
  require_contains "$TMPDIR/help.out" "feature-proof|ci|full"
  require_not_contains "$TMPDIR/help.out" "--basic"
  require_not_contains "$TMPDIR/help.out" "--app"
  require_not_contains "$TMPDIR/help.out" "--env"

  run_case() {
    local label="$1"
    local expected_workflow="$2"
    shift 2

    local log_file="$TMPDIR/$label.out"
    local run_id_file="$TMPDIR/$label.run-id"
    local summary_file="$TMPDIR/$label.summary.json"

    "$TEST_APP" "$@" --run-id-file "$run_id_file" --summary-file "$summary_file" --summary > "$log_file" 2>&1

    local run_id
    run_id="$(read_trimmed_file "$run_id_file")"
    require_non_empty "$run_id" "run id for $label"
    require_file "$summary_file"

    "$JQ" -e --arg workflow "$expected_workflow" '
      .kind == "workflow-summary"
      and .version == 1
      and .payload.workflow_id == $workflow
      and .payload.mode == "test"
      and .payload.exit_code == 0
      and (.payload.steps | type == "array")
      and (.payload.steps | length >= 1)
      and ([.payload.steps[] | .name | type] | all(. == "string"))
      and ([.payload.steps[] | .status | type] | all(. == "string"))
    ' "$summary_file" > /dev/null

    "$JQ" -e --arg runId "$run_id" --arg workflow "$expected_workflow" '
      select((.payload.runId // "") == $runId and (.payload.workflowId // "") == $workflow) | .payload.runId
    ' "$REGISTRY_ROOT/events.ndjson" > /dev/null

    require_contains "$log_file" "Summary"
    require_contains "$log_file" "OK: Exit code: 0"
  }

  run_case "mode-default" "workflow.test.full"
  run_case "mode-feature-proof" "workflow.test.feature-proof" --mode feature-proof
  run_case "mode-ci" "workflow.test.ci" --mode ci
  run_case "mode-full" "workflow.test.full" --mode full
  run_case "mode-ci-logging" "workflow.test.ci" --mode ci --log-level debug --output-mode both

  set +e
  "$TEST_APP" --mode nope --summary > "$TMPDIR/mode-invalid.out" 2>&1
  invalid_mode_rc="$?"
  set -e
  if [ "$invalid_mode_rc" -eq 0 ]; then
    fail "expected --mode nope to fail"
  fi
  require_contains "$TMPDIR/mode-invalid.out" "ERROR: unknown mode 'nope' (expected:"

  set +e
  "$TEST_APP" --basic > "$TMPDIR/basic-invalid.out" 2>&1
  invalid_basic_rc="$?"
  set -e
  if [ "$invalid_basic_rc" -eq 0 ]; then
    fail "expected --basic to fail"
  fi
  require_contains "$TMPDIR/basic-invalid.out" "ERROR: unknown option '--basic'"

  set +e
  "$TEST_APP" --mode ci --summary --log-level nope > "$TMPDIR/log-level-invalid.out" 2>&1
  invalid_log_level_rc="$?"
  set -e
  if [ "$invalid_log_level_rc" -eq 0 ]; then
    fail "expected --log-level nope to fail"
  fi
  require_contains "$TMPDIR/log-level-invalid.out" "ERROR: invalid --log-level 'nope'"

  set +e
  "$TEST_APP" --mode ci --summary --output-mode nope > "$TMPDIR/output-mode-invalid.out" 2>&1
  invalid_output_mode_rc="$?"
  set -e
  if [ "$invalid_output_mode_rc" -eq 0 ]; then
    fail "expected --output-mode nope to fail"
  fi
  require_contains "$TMPDIR/output-mode-invalid.out" "ERROR: invalid --output-mode 'nope'"

  echo "OK: test CLI modes resolve framework workflows through the public app surface" > "$out"
''
