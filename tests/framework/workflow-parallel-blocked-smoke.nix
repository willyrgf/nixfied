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
  runtimeMetadataFile = pkgs.writeText "nixfied-runtime-metadata-blocked-workflow.json" (
    builtins.toJSON {
      schema = {
        kind = "nixfied-runtime-metadata";
        version = 1;
      };
      tasks = { };
      workflows."workflow.test.parallel.blocked" = {
        mode = "test";
        family = "test";
        artifactsRoot = "";
        ephemeralEnabled = true;
        logging = {
          levelDefault = "";
          outputDefault = "";
        };
        failFast = false;
        parallelEnabled = true;
        maxWorkers = 2;
        lockPolicy = "exclusive";
        writeSummary = false;
        postRunAlways = false;
        closureSelectedServices = [ ];
        unitClosureSelectedServices = [ ];
        referenceClosureSelectedServices = [ ];
        phases = {
          preRun = {
            tasks = [ ];
            serviceSets = [ ];
          };
          postRun = {
            tasks = [ ];
            serviceSets = [ ];
          };
        };
        plan = [
          {
            name = "a";
            taskId = "task.test.parallel.blocked-a";
            needs = [ "b" ];
            locks = [ ];
            requiredServices = [ ];
            skipIfMissingEnv = [ ];
            when = {
              envPresent = [ ];
              envEquals = { };
            };
            selectedServices = [ ];
            produces = {
              artifacts = [ ];
              stateKeys = [ ];
            };
          }
          {
            name = "b";
            taskId = "task.test.parallel.blocked-b";
            needs = [ "a" ];
            locks = [ ];
            requiredServices = [ ];
            skipIfMissingEnv = [ ];
            when = {
              envPresent = [ ];
              envEquals = { };
            };
            selectedServices = [ ];
            produces = {
              artifacts = [ ];
              stateKeys = [ ];
            };
          }
        ];
      };
      workflowFamilies.test.modes = [ "parallel" ];
      availableServices = [ ];
    }
  );
  skippedServicesFile = pkgs.writeText "nixfied-workflow-skipped-services.txt" "";
  taskAdapter = pkgs.writeShellScript "nixfied-blocked-workflow-task-adapter" ''
    set -euo pipefail
    touch "$TMPDIR/task-adapter-invoked"
    echo "unexpected task adapter invocation: $*" >&2
    exit 99
  '';
  serviceSetAdapter = pkgs.writeShellScript "nixfied-blocked-workflow-service-set-adapter" ''
    set -euo pipefail
    touch "$TMPDIR/service-set-adapter-invoked"
    echo "unexpected service-set adapter invocation: $*" >&2
    exit 99
  '';
in
pkgs.runCommand "workflow-parallel-blocked-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  KERNEL=${kernelPackage}/bin/nixfied-kernel
  JQ=${pkgs.jq}/bin/jq
  ROOT="$TMPDIR/registry"
  RUN_ID="run-blocked"
  ATTEMPT_ID="attempt-blocked"
  WORKFLOW_ID="workflow.test.parallel.blocked"
  EVENTS_FILE="$ROOT/events.ndjson"

  mkdir -p "$ROOT"

  set +e
  "$KERNEL" workflow run \
    ${pkgs.lib.escapeShellArg runtimeMetadataFile} \
    ${pkgs.lib.escapeShellArg validationBundleFile} \
    "$ROOT" \
    "$RUN_ID" \
    "$ATTEMPT_ID" \
    "$WORKFLOW_ID" \
    ${pkgs.lib.escapeShellArg skippedServicesFile} \
    ${pkgs.lib.escapeShellArg taskAdapter} \
    ${pkgs.lib.escapeShellArg serviceSetAdapter} \
    true \
    2 > "$TMPDIR/workflow.out" 2>&1
  result_rc="$?"
  set -e
  [ "$result_rc" -eq 1 ] || fail "expected blocked workflow run to exit 1, got '$result_rc'"

  require_file "$EVENTS_FILE"
  require_not_file "$TMPDIR/task-adapter-invoked"
  require_not_file "$TMPDIR/service-set-adapter-invoked"

  "$JQ" -s -e '
    map(select(.payload.runId == $run_id)) as $events
    | ($events | map(select((.payload.taskId // "") == "" and .payload.workflowId == $workflow_id) | .payload.state) == ["queued", "failed"])
      and ($events | map(select((.payload.taskId // "") != "") | .payload.taskId) == ["task.test.parallel.blocked-a", "task.test.parallel.blocked-b"])
      and ($events | map(select((.payload.taskId // "") != "") | .payload.state) == ["canceled", "canceled"])
      and ($events | map(select((.payload.taskId // "") != "") | (.payload.detail.reason // "")) == ["blocked", "blocked"])
  ' --arg run_id "$RUN_ID" --arg workflow_id "$WORKFLOW_ID" "$EVENTS_FILE" > /dev/null \
    || fail "blocked workflow run did not record the expected canceled task events"

  echo "OK: workflow run cancels blocked parallel units without invoking shell task drivers" > "$out"
''
