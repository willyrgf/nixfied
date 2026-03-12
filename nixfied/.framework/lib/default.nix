# Framework helpers - aggregator
# Re-exports the same flat attrset as the original lib.nix
{
  pkgs,
  project,
  hooks ? { },
  commandSurfaces ? null,
  featureInventory ? null,
}:

let
  shellContract = import ../../framework/runtime/helpers/shell-contract.nix { inherit pkgs; };
  baseLoggingPrelude =
    (import ../../framework/runtime/helpers/helpers.nix {
      inherit pkgs project;
      hooks = { };
      summaryParser = "";
    }).loggingPrelude;
  summary = import ./summary.nix {
    inherit pkgs project;
    loggingPrelude = baseLoggingPrelude;
  };
  helpers = import ../../framework/runtime/helpers/helpers.nix {
    inherit pkgs project hooks;
    inherit (summary) summaryParser;
  };
  fixtures = import ../../framework/runtime/helpers/fixtures.nix {
    inherit pkgs project;
  };
  builders = import ../../framework/runtime/helpers/builders.nix {
    inherit pkgs project;
    inherit shellContract;
    fixtureLib = fixtures;
    inherit (helpers)
      loadEnv
      loadEnvFile
      helpersScript
      hookExports
      ;
  };
  appApi = import ./app-api.nix {
    inherit pkgs;
    inherit shellContract;
    inherit (builders) mkApp;
  };
  serviceApi = import ./service-api.nix {
    inherit
      pkgs
      appApi
      shellContract
      ;
  };
  discovery = import ../../framework/runtime/helpers/discovery.nix {
    inherit pkgs project;
    inherit (helpers) loggingPrelude;
    inherit
      commandSurfaces
      featureInventory
      ;
  };
  process = import ../../framework/runtime/helpers/process.nix {
    inherit pkgs;
    inherit (helpers) loggingPrelude;
  };
  id = import ../../framework/runtime/helpers/id.nix {
    inherit pkgs project;
    inherit (helpers) loggingPrelude;
  };
  runtimeEvents = import ../../framework/runtime/helpers/runtime-events.nix {
    inherit pkgs project;
    inherit (helpers) loggingPrelude;
  };
  managedServiceLifecycle = import ./managed-service-lifecycle.nix { inherit pkgs; };
  slotEnvRuntime = import ../../framework/runtime/helpers/slot-env-runtime.nix { inherit pkgs; };
  servicePolicy = import ../../framework/runtime/helpers/service-policy.nix { inherit pkgs; };
  portUtils = import ../../framework/runtime/helpers/port-utils.nix {
    inherit pkgs;
    inherit (helpers) loggingPrelude;
  };
  parallel = import ../../framework/runtime/helpers/parallel.nix {
    inherit pkgs;
    inherit (helpers) loggingPrelude;
  };
  runRegistry = import ../../framework/runtime/helpers/run-registry.nix {
    inherit pkgs project;
    inherit (helpers) loggingPrelude;
  };
in
{
  inherit (helpers)
    loadEnv
    loadEnvFile
    loggingPrelude
    helpersScript
    hookExports
    ;
  inherit (summary) summaryParser;
  inherit (builders)
    withTiming
    mkAppScript
    mkApp
    mkAppWithDeps
    ;
  inherit fixtures;
  inherit shellContract;
  inherit appApi;
  inherit serviceApi;
  inherit discovery;
  inherit managedServiceLifecycle;
  inherit (process) mkSignalHandler mkProcessManager;
  inherit (id)
    mkPlanId
    mkUniqueId
    mkRunId
    resolveId
    ;
  inherit (runtimeEvents)
    emitEvent
    serviceEvents
    serviceLogs
    serviceStatus
    registryRoot
    ;
  inherit slotEnvRuntime;
  inherit servicePolicy;
  inherit (portUtils) mkPortCleanup mkPortConflictChecker;
  inherit (parallel) mkParallelRunner;
  inherit (runRegistry) runRegistryStart;
}
