{
  pkgs,
  registry,
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  runtimeMaterialization = import ./lib/runtime-materialization.nix;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
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
          nixfied.services.postgres.enable = lib.mkForce true;
          nixfied.services.nginx.enable = lib.mkForce true;
          nixfied.services.minio.enable = lib.mkForce false;
          nixfied.services.reth.enable = lib.mkForce false;
          nixfied.services.helios.enable = lib.mkForce false;

          nixfied.services.postgres.probes = {
            health = {
              strategy = "replace";
              steps = [
                {
                  kind = "exec";
                  command = ''
                    echo "MARKER: postgres health"
                  '';
                }
              ];
            };
            ready = {
              strategy = "replace";
              steps = [
                {
                  kind = "exec";
                  command = ''
                    echo "MARKER: postgres ready"
                  '';
                }
              ];
            };
          };

          nixfied.services.nginx.probes = {
            health = {
              strategy = "replace";
              steps = [
                {
                  kind = "exec";
                  command = ''
                    echo "MARKER: nginx health"
                  '';
                }
              ];
            };
            ready = {
              strategy = "replace";
              steps = [
                {
                  kind = "exec";
                  command = ''
                    echo "MARKER: nginx ready"
                  '';
                }
              ];
            };
          };

          nixfied.tasks."test.workflow.probe-scope" = {
            id = probeTaskId;
            summary = "Workflow probe scope unit";
            description = "Used to validate workflow-scoped ready and health probes.";
            runner.command = ''
              set -euo pipefail
              echo "MARKER: workflow unit"
            '';
          };

          nixfied.workflows."test.workflow.probe-scope" = {
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
    model = compiled.model;
    services = compiled.services;
    projectRoot = ../..;
    inherit
      (runtimeMaterialization {
        inherit pkgs;
        model = compiled.model;
        services = compiled.services;
      })
      serviceHookEnv
      serviceSetPrograms
      ;
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
