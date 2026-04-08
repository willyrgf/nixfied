{
  pkgs,
  runtimeProgram,
  model,
  contractBundle,
}:
let
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };

  runtimeProgramPath = builtins.toString runtimeProgram;
  modelApps = model.apps or { };

  mkRuntimeExecApp =
    appName: command:
    mkShellApp {
      inherit appName;
      body = ''
        exec ${pkgs.lib.escapeShellArg runtimeProgramPath} ${command} "$@"
      '';
    };

  directApps = builtins.mapAttrs (
    appId: app:
    if (app.kind or "") == "taskRef" then
      mkRuntimeExecApp appId "run-task ${pkgs.lib.escapeShellArg app.taskId}"
    else if (app.kind or "") == "workflowRef" then
      mkRuntimeExecApp appId "run-workflow ${pkgs.lib.escapeShellArg app.workflowId}"
    else if (app.kind or "") == "serviceOp" then
      mkRuntimeExecApp appId "run-service ${pkgs.lib.escapeShellArg app.service} ${pkgs.lib.escapeShellArg app.operation}"
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
  "run-task" = mkRuntimeExecApp "run-task" "run-task";
  "run-workflow" = mkRuntimeExecApp "run-workflow" "run-workflow";
  "run-workflow-parallel" = mkRuntimeExecApp "run-workflow-parallel" "run-workflow-parallel";
  "runs" = mkRuntimeExecApp "runs" "runs";
  "stop-run" = mkRuntimeExecApp "stop-run" "stop-run";
  "stop-all-runs" = mkRuntimeExecApp "stop-all-runs" "stop-all-runs";
}
