{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  kernelPackage = import ../../nixfied/framework/runtime/kernel { inherit pkgs; };
  workflowPlanFile = pkgs.writeText "nixfied-workflow-scheduler-plan-blocked.json" (
    builtins.toJSON {
      kind = "nixfied-workflow-scheduler-plan";
      version = 1;
      workflows."workflow.test.parallel.blocked".units = [
        {
          name = "a";
          taskId = "task.test.parallel.blocked-a";
          needs = [ "b" ];
          selectedServicesCsv = "";
          producesJson = "{}";
        }
        {
          name = "b";
          taskId = "task.test.parallel.blocked-b";
          needs = [ "a" ];
          selectedServicesCsv = "";
          producesJson = "{}";
        }
      ];
    }
  );
  skippedServicesFile = pkgs.writeText "nixfied-workflow-skipped-services.txt" "";
in
pkgs.runCommand "workflow-parallel-blocked-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  KERNEL=${kernelPackage}/bin/nixfied-kernel
  STATE_FILE="$TMPDIR/workflow-parallel-state.json"

  "$KERNEL" workflow parallel-init ${pkgs.lib.escapeShellArg workflowPlanFile} ${pkgs.lib.escapeShellArg skippedServicesFile} workflow.test.parallel.blocked false 2 "$STATE_FILE" > /dev/null

  first_action="$("$KERNEL" workflow parallel-step "$STATE_FILE" "" "" "" "" "" "")"
  [ "$first_action" = "$(printf 'cancel\037a\037task.test.parallel.blocked-a\037blocked\037\037')" ] || fail "expected first blocked cancel action, got '$first_action'"

  ${pkgs.jq}/bin/jq -e '
    .workflowStatus == 1
    and .stopScheduling == false
    and .units.a.state == "cancel-pending"
    and .units.a.cancelReason == "blocked"
    and .units.b.state == "cancel-pending"
    and .units.b.cancelReason == "blocked"
  ' "$STATE_FILE" > /dev/null || fail "blocked scheduler state was not persisted"

  "$KERNEL" workflow parallel-step "$STATE_FILE" a canceled "" blocked "" "" > /dev/null

  second_action="$("$KERNEL" workflow parallel-step "$STATE_FILE" "" "" "" "" "" "")"
  [ "$second_action" = "$(printf 'cancel\037b\037task.test.parallel.blocked-b\037blocked\037\037')" ] || fail "expected second blocked cancel action, got '$second_action'"

  "$KERNEL" workflow parallel-step "$STATE_FILE" b canceled "" blocked "" "" > /dev/null

  final_action="$("$KERNEL" workflow parallel-step "$STATE_FILE" "" "" "" "" "" "")"
  [ "$final_action" = "$(printf 'done\0371')" ] || fail "expected blocked workflow to finish with status 1, got '$final_action'"

  echo "OK: parallel scheduler cancels dead-end units with blocked reason and exits non-zero" > "$out"
''
