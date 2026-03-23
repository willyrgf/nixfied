{
  pkgs,
  registry,
}:
let
  snapshot = registry.snapshot.fromEvents [
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "task.dev";
        workflowId = "workflow.ci.full";
        state = "running";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "task.dev";
        workflowId = "workflow.ci.full";
        state = "passed";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "";
        workflowId = "workflow.ci.full";
        state = "passed";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "";
        workflowId = "";
        state = "ready";
      };
    }
  ];
in
assert snapshot."task:task.dev" == "passed";
assert snapshot."workflow:workflow.ci.full" == "passed";
assert (!builtins.hasAttr "workflow:" snapshot);
pkgs.runCommand "registry-replay" { } ''
  echo "OK: registry replay snapshot is deterministic" > "$out"
''
