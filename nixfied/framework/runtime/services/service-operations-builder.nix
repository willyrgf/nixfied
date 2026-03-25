# Service operations builder -- builds the standard operations attrset
# (init, preflight-start, start, stop, restart, status, health,
# check-config, full-start, full-start-test, ready) from a lifecycle,
# then merges any service-specific extras on top.
#
# Usage:
#   ops = import ../service-operations-builder.nix {
#     displayName = "Reth";
#     lifecycle = lifecycle;
#     extraOperations = { ... };
#   };
{
  displayName,
  lifecycle,
  extraOperations ? { },
}:

let
  base = {
    init = {
      script = lifecycle.init;
      summary = "Initialize ${displayName} runtime directories";
      details = "Creates ${displayName} runtime directories for the current slot/environment.";
    };
    preflight-start = {
      script = lifecycle.preflightStart;
      summary = "Validate ${displayName} start preconditions";
      details = "Checks deterministic blockers before ${displayName} startup for the current slot/environment.";
      exposeApp = false;
      exposeHook = false;
    };
    start = {
      script = lifecycle.startLeaf;
      preOps = [
        "init"
        "check-config"
        "preflight-start"
      ];
      summary = "Start ${displayName}";
      details = "Starts ${displayName} for the current slot/environment.";
    };
    stop = {
      script = lifecycle.stop;
      summary = "Stop ${displayName}";
      details = "Stops ${displayName} for the current slot/environment.";
    };
    restart = {
      preOps = [
        "stop"
        "start"
      ];
      summary = "Restart ${displayName}";
      details = "Stops then starts ${displayName} for the current slot/environment.";
    };
    status = {
      script = lifecycle.status;
      summary = "Show ${displayName} status";
      details = "Prints ${displayName} status for the current slot/environment.";
    };
    health = {
      script = lifecycle.health;
      summary = "Run ${displayName} health check";
      details = "Checks ${displayName} health for the current slot/environment.";
    };
    check-config = {
      script = lifecycle.checkConfig;
      summary = "Validate ${displayName} configuration";
      details = "Validates ${displayName} configuration for the current slot/environment.";
    };
    full-start = {
      script = lifecycle.fullStartLeaf;
      hook = "FULL_START";
      preOps = [
        "init"
        "check-config"
        "preflight-start"
      ];
      summary = "Init/check/start ${displayName}";
      details = "Performs init + check-config + start for ${displayName}.";
    };
    full-start-test = {
      script = lifecycle.fullStartTestLeaf;
      hook = "FULL_START_TEST";
      preOps = [
        "init"
        "check-config"
        "preflight-start"
      ];
      summary = "Init/check/start ${displayName} for test profile";
      details = "Performs init + check-config + start for ${displayName} (test profile).";
    };
    ready = {
      script = lifecycle.ready;
      hook = "READY";
      summary = "Wait for ${displayName} readiness";
      details = "Waits for ${displayName} to be ready for the current slot/environment.";
    };
  };
in
base // extraOperations
