{
  pkgs,
  model,
}:
let
  workflow = model.workflows."workflow.ci.full";
  order = map (unit: unit.taskId) workflow.plan;
  expected = [
    "task.ci.quality"
    "task.ci.tests"
    "task.ci.nginx-proxy"
    "task.ci.system-quick"
  ];
in
assert order == expected;
pkgs.runCommand "v2-scheduler-order" { } ''
  echo "OK: scheduler order is deterministic" > "$out"
''
