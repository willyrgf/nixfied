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
assert pkgs.lib.hasInfix "registry_next_seq()" source;
assert pkgs.lib.hasInfix "registry_events_snapshot()" source;
assert pkgs.lib.hasInfix "registry_lock_acquire_shared()" source;
assert pkgs.lib.hasInfix "registry_seq_file()" source;
assert pkgs.lib.hasInfix "registry_events_file()" source;
assert pkgs.lib.hasInfix "registry_events_lock_file()" source;
assert pkgs.lib.hasInfix "REGISTRY_EVENT_SCHEMA_VERSION" source;
assert pkgs.lib.hasInfix "REGISTRY_TIMESTAMP_FORMAT" source;
assert pkgs.lib.hasInfix "\${pkgs.procps}/bin/ps" source;
assert pkgs.lib.hasInfix "events.ndjson" source;
assert pkgs.lib.hasInfix "flock -w" source;
assert pkgs.lib.hasInfix "registryTimestampFormat = \"%Y-%m-%dT%H:%M:%SZ\";" source;
assert pkgs.lib.hasInfix "schemaVersion: $schemaVersion" source;
assert pkgs.lib.hasInfix "workflowId: $workflowId" source;
pkgs.runCommand "registry-events-contract" { } ''
  echo "OK: registry event contracts are stable" > "$out"
''
