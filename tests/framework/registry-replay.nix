{
  pkgs,
  registry,
}:
let
  snapshot = registry.snapshot.fromEvents [
    {
      taskId = "task.dev";
      workflowId = "workflow.ci.full";
      state = "running";
    }
    {
      taskId = "task.dev";
      workflowId = "workflow.ci.full";
      state = "passed";
    }
    {
      taskId = "";
      workflowId = "workflow.ci.full";
      state = "passed";
    }
    {
      taskId = "";
      workflowId = "";
      state = "ready";
    }
  ];
in
assert snapshot."task:task.dev" == "passed";
assert snapshot."workflow:workflow.ci.full" == "passed";
assert (!builtins.hasAttr "workflow:" snapshot);
pkgs.runCommand "registry-replay" { } ''
  echo "OK: registry replay snapshot is deterministic" > "$out"
''
