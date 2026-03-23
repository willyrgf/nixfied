{
  pkgs,
  registry,
}:
let
  source = builtins.readFile ../../nixfied/framework/runtime/registry/events.nix;
  snapshot = registry.snapshot.fromEvents [
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "task.ci.quality";
        workflowId = "workflow.ci.full";
        state = "running";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "task.ci.quality";
        workflowId = "workflow.ci.full";
        state = "passed";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "task.ci.tests";
        workflowId = "workflow.ci.full";
        state = "failed";
      };
    }
    {
      kind = "runtime-event";
      version = 1;
      payload = {
        taskId = "";
        workflowId = "workflow.ci.full";
        state = "failed";
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
assert snapshot."task:task.ci.quality" == "passed";
assert snapshot."task:task.ci.tests" == "failed";
assert snapshot."workflow:workflow.ci.full" == "failed";
assert (!builtins.hasAttr "workflow:" snapshot);
assert pkgs.lib.hasInfix "registryLocksShell = import ./events-locks.nix" source;
assert pkgs.lib.hasInfix "registrySnapshotShell = import ./events-snapshot.nix" source;
assert pkgs.lib.hasInfix "registryAppendShell = import ./events-append.nix" source;
assert pkgs.lib.hasInfix "\${registryLocksShell}" source;
assert pkgs.lib.hasInfix "\${registrySnapshotShell}" source;
assert pkgs.lib.hasInfix "\${registryAppendShell}" source;
assert pkgs.lib.hasInfix "REGISTRY_EVENT_KIND" source;
assert pkgs.lib.hasInfix "REGISTRY_EVENT_VERSION" source;
assert pkgs.lib.hasInfix "REGISTRY_TIMESTAMP_FORMAT" source;
assert pkgs.lib.hasInfix "events.ndjson" source;
assert pkgs.lib.hasInfix "registryTimestampFormat = \"%Y-%m-%dT%H:%M:%SZ\";" source;
assert pkgs.lib.hasInfix "registryEventPayloadExpr =" source;
assert pkgs.lib.hasInfix "kind: $kind, version: $version, payload:" source;
pkgs.runCommand "registry-events-contract" { } ''
  echo "OK: registry event contracts are stable" > "$out"
''
