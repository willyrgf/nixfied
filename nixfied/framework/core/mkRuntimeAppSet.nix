{
  pkgs,
  orchestratorProgram,
  serviceDispatcherProgram,
  model,
  contractBundle,
}:
let
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };

  orchestratorProgramPath = builtins.toString orchestratorProgram;
  serviceDispatcherProgramPath = builtins.toString serviceDispatcherProgram;
  modelApps = model.apps or { };

  mkOrchestratorExecApp =
    appName: body:
    mkShellApp {
      inherit appName;
      body = ''
        exec ${pkgs.lib.escapeShellArg orchestratorProgramPath} ${body} "$@"
      '';
    };

  mkServiceExecApp =
    appName: serviceName: operation:
    mkShellApp {
      inherit appName;
      body = ''
        exec ${pkgs.lib.escapeShellArg serviceDispatcherProgramPath} run-service ${pkgs.lib.escapeShellArg serviceName} ${pkgs.lib.escapeShellArg operation} "$@"
      '';
    };

  directApps = builtins.mapAttrs (
    appId: app:
    if (app.kind or "") == "taskRef" then
      mkOrchestratorExecApp appId "run-task ${pkgs.lib.escapeShellArg app.taskId}"
    else if (app.kind or "") == "workflowRef" then
      mkOrchestratorExecApp appId "run-workflow ${pkgs.lib.escapeShellArg app.workflowId}"
    else if (app.kind or "") == "serviceOp" then
      mkServiceExecApp appId app.service app.operation
    else if (app.kind or "") == "machineOutput" then
      null
    else
      throw "mkRuntimeAppSet: unsupported app kind '${app.kind or ""}' for '${appId}'"
  ) modelApps;

  appSet =
    let
      machineOutputIds = builtins.filter (appId: (modelApps.${appId}.kind or "") == "machineOutput") (
        builtins.attrNames modelApps
      );
      apps = (builtins.removeAttrs directApps machineOutputIds) // machineOutputApps;
      machineOutputApps = builtins.listToAttrs (
        map (
          appId:
          let
            app = modelApps.${appId};
          in
          {
            name = appId;
            value = {
              type = "app";
              program = toString (
                import ./mkMachineOutputPrograms.nix {
                  inherit
                    pkgs
                    appId
                    app
                    contractBundle
                    ;
                  targetProgram = apps.${app.targetAppId}.program;
                  setupPrograms = map (setupAppId: apps.${setupAppId}.program) (app.setupAppIds or [ ]);
                  teardownPrograms = map (teardownAppId: apps.${teardownAppId}.program) (app.teardownAppIds or [ ]);
                }
              );
            };
          }
        ) machineOutputIds
      );
    in
    apps;
in
appSet
// {
  "run-task" = mkOrchestratorExecApp "run-task" "run-task";
  "run-workflow" = mkOrchestratorExecApp "run-workflow" "run-workflow";
  "run-workflow-parallel" = mkOrchestratorExecApp "run-workflow-parallel" "run-workflow-parallel";
  "runs" = mkOrchestratorExecApp "runs" "runs";
  "stop-run" = mkOrchestratorExecApp "stop-run" "stop-run";
  "stop-all-runs" = mkOrchestratorExecApp "stop-all-runs" "stop-all-runs";
}
