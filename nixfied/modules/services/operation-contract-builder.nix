{
  displayName,
  extraOperations ? { },
}:
let
  base = {
    init = {
      runtimeOp = "init";
      summary = "Initialize ${displayName} runtime directories";
      details = "Creates ${displayName} runtime directories for the current slot/environment.";
    };

    preflight-start = {
      runtimeOp = "preflight-start";
      summary = "Validate ${displayName} start preconditions";
      details = "Checks deterministic blockers before ${displayName} startup for the current slot/environment.";
      exposeApp = false;
      exposeHook = false;
    };

    start = {
      runtimeOp = "start-leaf";
      preOps = [
        "init"
        "check-config"
        "preflight-start"
      ];
      summary = "Start ${displayName}";
      details = "Starts ${displayName} for the current slot/environment.";
    };

    stop = {
      runtimeOp = "stop";
      summary = "Stop ${displayName}";
      details = "Stops ${displayName} for the current slot/environment.";
    };

    restart = {
      runtimeOp = null;
      preOps = [
        "stop"
        "start"
      ];
      summary = "Restart ${displayName}";
      details = "Stops then starts ${displayName} for the current slot/environment.";
    };

    status = {
      runtimeOp = "status";
      summary = "Show ${displayName} status";
      details = "Prints ${displayName} status for the current slot/environment.";
    };

    health = {
      runtimeOp = "health";
      summary = "Run ${displayName} health check";
      details = "Checks ${displayName} health for the current slot/environment.";
    };

    check-config = {
      runtimeOp = "check-config";
      summary = "Validate ${displayName} configuration";
      details = "Validates ${displayName} configuration for the current slot/environment.";
    };

    full-start = {
      runtimeOp = "full-start-leaf";
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
      runtimeOp = "full-start-test-leaf";
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
      runtimeOp = "ready";
      hook = "READY";
      summary = "Wait for ${displayName} readiness";
      details = "Waits for ${displayName} to be ready for the current slot/environment.";
    };
  };
in
base // extraOperations
