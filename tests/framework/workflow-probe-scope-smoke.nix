{
  pkgs,
  registry,
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };

  probeTaskId = "task.test.workflow.probe-scope";
  probeWorkflowId = "workflow.test.workflow.probe-scope";

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied = {
            services = {
              postgres = {
                enable = lib.mkForce true;
                checks = {
                  health.steps = [
                    {
                      kind = "exec";
                      command = ''
                        echo "MARKER: postgres health"
                      '';
                    }
                  ];
                  ready.steps = [
                    {
                      kind = "exec";
                      command = ''
                        echo "MARKER: postgres ready"
                      '';
                    }
                  ];
                };
              };
              nginx = {
                enable = lib.mkForce true;
                checks = {
                  health.steps = [
                    {
                      kind = "exec";
                      command = ''
                        echo "MARKER: nginx health"
                      '';
                    }
                  ];
                  ready.steps = [
                    {
                      kind = "exec";
                      command = ''
                        echo "MARKER: nginx ready"
                      '';
                    }
                  ];
                };
              };
              minio.enable = lib.mkForce false;
              reth.enable = lib.mkForce false;
              helios.enable = lib.mkForce false;
            };
            tasks."test.workflow.probe-scope" = {
              id = probeTaskId;
              summary = "Workflow probe scope unit";
              description = "Used to validate workflow-scoped ready and health probes.";
              runner.command = ''
                set -euo pipefail
                echo "MARKER: workflow unit"
              '';
            };
            workflows."test.workflow.probe-scope" = {
              id = probeWorkflowId;
              summary = "Workflow probe scope smoke";
              description = "Exercises workflow-scoped ready and health probes. The workflow should only probe its unit requirements.";
              mode = "custom";
              maxWorkers = 1;
              units.probe = {
                taskId = probeTaskId;
                needs = [ ];
                locks = [ ];
                when = {
                  envEquals = { };
                  envPresent = [ ];
                };
                skipIfMissingEnv = [ ];
                requirements.services = [ "postgres" ];
              };
              stages = [ ];
              preRun.tasks = [ "task.ops.ready" ];
              postRun = {
                tasks = [ "task.ops.health" ];
                alwaysRun = true;
              };
            };
          };
        }
      )
    ];
    localOverrides = [ ];
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    inherit (compiled) model services;
    projectRoot = ../..;
    inherit (runtimeDeps) serviceDispatcherProgram;
    runtimeBin = runtimeDeps.serviceDispatcherProgram;
  };

  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit (compiled) model services serviceDefinitions;
  };
in
assert
  compiled.model.compiled.execution.workflows.byId.${probeWorkflowId}.unitClosureSelectedServices
  == [ "postgres" ];
pkgs.runCommand "workflow-probe-scope-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export NIXFIED_ORCHESTRATOR_RUN_ID="workflow-probe-scope-smoke"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR"

  "$EXECUTOR" run-workflow "${probeWorkflowId}" --summary > "$TMPDIR/workflow.out" 2>&1

  require_contains "$TMPDIR/workflow.out" "MARKER: workflow unit"
  require_contains "$TMPDIR/workflow.out" "MARKER: postgres ready"
  require_contains "$TMPDIR/workflow.out" "MARKER: postgres health"
  require_not_contains "$TMPDIR/workflow.out" "MARKER: nginx ready"
  require_not_contains "$TMPDIR/workflow.out" "MARKER: nginx health"

  echo "OK: workflow probe scope uses unit-closure service selection" > "$out"
''
