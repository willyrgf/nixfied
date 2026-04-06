{ pkgs, model }:
let
  workflow = model.workflows."workflow.test.framework.selfhost";
  task = model.tasks."task.test.framework.selfhost";
  inherit (task.runner) command;
in
assert workflow.units.main.taskId == "task.test.framework.selfhost";
assert workflow.execution.ephemeral.enable;
assert task.runner.type == "shell";
assert pkgs.lib.hasInfix ''"$NIXFIED_EXECUTOR_SELF" run-task task.dev'' command;
assert pkgs.lib.hasInfix ''"$NIXFIED_EXECUTOR_SELF" run-workflow workflow.ci.basic --summary''
  command;
assert !pkgs.lib.hasInfix ''nix run "path:$ROOT"#run-task -- task.dev'' command;
pkgs.runCommand "framework-selfhost-contract" { } ''
  echo "OK: framework self-host wiring reuses the built executor closure" > "$out"
''
