{
  pkgs,
  registry,
}:
let
  source = builtins.readFile ../../nixfied/registry/events.nix;
  snapshot = registry.snapshot.fromEvents [
    {
      taskId = "task.ci.quality";
      workflowId = "workflow.ci.full";
      state = "running";
    }
    {
      taskId = "task.ci.quality";
      workflowId = "workflow.ci.full";
      state = "passed";
    }
    {
      taskId = "task.ci.tests";
      workflowId = "workflow.ci.full";
      state = "failed";
    }
    {
      taskId = "";
      workflowId = "workflow.ci.full";
      state = "failed";
    }
  ];
in
assert snapshot."task:task.ci.quality" == "passed";
assert snapshot."task:task.ci.tests" == "failed";
assert snapshot."workflow:workflow.ci.full" == "failed";
assert pkgs.lib.hasInfix "registryLocksShell = import ./events-locks.nix" source;
assert pkgs.lib.hasInfix "registrySnapshotShell = import ./events-snapshot.nix" source;
assert pkgs.lib.hasInfix "registryAppendShell = import ./events-append.nix" source;
assert pkgs.lib.hasInfix "\${registryLocksShell}" source;
assert pkgs.lib.hasInfix "\${registrySnapshotShell}" source;
assert pkgs.lib.hasInfix "\${registryAppendShell}" source;
assert pkgs.lib.hasInfix "REGISTRY_EVENT_SCHEMA_VERSION" source;
assert pkgs.lib.hasInfix "REGISTRY_TIMESTAMP_FORMAT" source;
assert pkgs.lib.hasInfix "events.ndjson" source;
assert pkgs.lib.hasInfix "registryTimestampFormat = \"%Y-%m-%dT%H:%M:%SZ\";" source;
assert pkgs.lib.hasInfix "registryEventPayloadExpr =" source;
pkgs.runCommand "registry-events-contract" { } ''
  echo "OK: registry event contracts are stable" > "$out"
''
