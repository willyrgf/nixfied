{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  kernelPackage = import ../../nixfied/framework/runtime/kernel { inherit pkgs; };
  runtimeArtifactContracts = import ../../nixfied/framework/contracts/runtime-artifact-contracts.nix {
    inherit pkgs;
  };
  validationBundleFile = pkgs.writeText "nixfied-runtime-artifact-contract-bundle.json" (
    builtins.toJSON runtimeArtifactContracts.bundle
  );
  summaryPlanFile = pkgs.writeText "nixfied-workflow-summary-plan.json" (
    builtins.toJSON {
      kind = "nixfied-workflow-summary-plan";
      version = 1;
      taskRunnerTypes = {
        "task.skip" = "shell";
        "task.fail" = "shell";
      };
    }
  );
in
pkgs.runCommand "registry-detail-derivation-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  KERNEL=${kernelPackage}/bin/nixfied-kernel
  ROOT="$TMPDIR/registry"
  RUN_ID="run-123"
  ATTEMPT_ID="attempt-123"
  WORKFLOW_ID="workflow.test"
  EXPORT_FILE="$TMPDIR/registry.exports"
  SUMMARY_EXPORTS="$TMPDIR/summary.exports"
  INDEX_FILE="$ROOT/events.index.tsv"
  STEPS_FILE="$TMPDIR/steps.tsv"

  mkdir -p "$ROOT"

  "$KERNEL" event-detail render slotLifecycle --mode task > "$TMPDIR/task-mode.detail.json"
  "$KERNEL" event-detail render serviceLifecycle --reason service-skipped --service-name postgres > "$TMPDIR/task-skip.detail.json"
  "$KERNEL" event-detail render slotLifecycle --exit-code 7 > "$TMPDIR/task-fail.detail.json"

  "$KERNEL" registry append ${pkgs.lib.escapeShellArg validationBundleFile} "$ROOT" "$RUN_ID" "$ATTEMPT_ID" "$WORKFLOW_ID" "task.skip" "queued" "$TMPDIR/task-mode.detail.json" "$EXPORT_FILE" > /dev/null
  "$KERNEL" registry append ${pkgs.lib.escapeShellArg validationBundleFile} "$ROOT" "$RUN_ID" "$ATTEMPT_ID" "$WORKFLOW_ID" "task.skip" "canceled" "$TMPDIR/task-skip.detail.json" "$EXPORT_FILE" > /dev/null
  "$KERNEL" registry append ${pkgs.lib.escapeShellArg validationBundleFile} "$ROOT" "$RUN_ID" "$ATTEMPT_ID" "$WORKFLOW_ID" "task.fail" "queued" "$TMPDIR/task-mode.detail.json" "$EXPORT_FILE" > /dev/null
  "$KERNEL" registry append ${pkgs.lib.escapeShellArg validationBundleFile} "$ROOT" "$RUN_ID" "$ATTEMPT_ID" "$WORKFLOW_ID" "task.fail" "running" "$TMPDIR/task-mode.detail.json" "$EXPORT_FILE" > /dev/null
  "$KERNEL" registry append ${pkgs.lib.escapeShellArg validationBundleFile} "$ROOT" "$RUN_ID" "$ATTEMPT_ID" "$WORKFLOW_ID" "task.fail" "failed" "$TMPDIR/task-fail.detail.json" "$EXPORT_FILE" > /dev/null

  require_file "$INDEX_FILE"

  skip_index="$(${pkgs.gawk}/bin/awk -F '\t' '$7 == "task.skip" && $8 == "canceled" { print $9 ":" $10 }' "$INDEX_FILE")"
  [ "$skip_index" = "service-skipped:" ] || fail "registry index should derive canceled reason from detail (got '$skip_index')"

  fail_index="$(${pkgs.gawk}/bin/awk -F '\t' '$7 == "task.fail" && $8 == "failed" { print $9 ":" $10 }' "$INDEX_FILE")"
  [ "$fail_index" = ":7" ] || fail "registry index should derive failed exit code from detail (got '$fail_index')"

  "$KERNEL" registry terminal "$INDEX_FILE" "$RUN_ID" "$ATTEMPT_ID" > "$TMPDIR/terminal.out"
  terminal_status="$(read_trimmed_file "$TMPDIR/terminal.out")"
  [ "$terminal_status" = "$(printf 'failed\t7')" ] || fail "registry terminal should preserve derived exit code (got '$terminal_status')"

  "$KERNEL" summary collect-steps ${pkgs.lib.escapeShellArg summaryPlanFile} "$INDEX_FILE" "$RUN_ID" "$ATTEMPT_ID" "$STEPS_FILE" "$SUMMARY_EXPORTS" > /dev/null
  . "$SUMMARY_EXPORTS"

  [ "$WORKFLOW_PASSED_COUNT" = "0" ] || fail "unexpected passed count '$WORKFLOW_PASSED_COUNT'"
  [ "$WORKFLOW_FAILED_COUNT" = "1" ] || fail "unexpected failed count '$WORKFLOW_FAILED_COUNT'"
  [ "$WORKFLOW_SKIPPED_COUNT" = "1" ] || fail "unexpected skipped count '$WORKFLOW_SKIPPED_COUNT'"
  [ "$WORKFLOW_CANCELED_COUNT" = "0" ] || fail "unexpected canceled count '$WORKFLOW_CANCELED_COUNT'"

  skip_step="$(${pkgs.gawk}/bin/awk -F '\t' '$1 == "task.skip" { print $2 ":" $4 ":" $7 ":" $8 }' "$STEPS_FILE")"
  [ "$skip_step" = "skipped:canceled:service-skipped:" ] || fail "summary collect-steps should derive skipped status from detail reason (got '$skip_step')"

  fail_step="$(${pkgs.gawk}/bin/awk -F '\t' '$1 == "task.fail" { print $2 ":" $4 ":" $8 }' "$STEPS_FILE")"
  [ "$fail_step" = "failed:failed:7" ] || fail "summary collect-steps should preserve derived exit code from detail (got '$fail_step')"

  echo "OK: registry terminal and summary steps derive reason and exit-code from validated detail" > "$out"
''
