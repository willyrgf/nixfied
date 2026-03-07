{ pkgs, model }:
let
  workflow = model.workflows."workflow.test.framework.selfhost";
  task = model.tasks."task.test.framework.selfhost";
  command = task.runner.command;
in
assert workflow.units.main.taskId == "task.test.framework.selfhost";
assert task.runner.type == "shell";
assert pkgs.lib.hasInfix ''nix run "path:$ROOT"#run-task -- task.dev'' command;
assert pkgs.lib.hasInfix ''nix run "path:$ROOT"#run-workflow -- workflow.ci.basic --summary''
  command;
pkgs.runCommand "framework-selfhost-contract" { } ''
  echo "OK: framework self-host wiring is composable and stable" > "$out"
''
