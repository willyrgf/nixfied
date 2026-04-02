{
  pkgs,
  model,
  services,
  runtimeHash ? model.identity.evalHash,
  projectRoot,
  registry,
  frameworkSourceFlakeRef ? null,
  appPrograms ? { },
  serviceSetPrograms ? { },
  serviceHookEnv ? { },
  executionEnabled ? true,
  includeRuntimeControlApps ? true,
}:
let
  lib = pkgs.lib;
  mkShellApp = import ../core/mk-shell-app.nix { inherit pkgs; };
  shellCommon = import ../core/shell-common.nix { inherit pkgs; };
  workspaceMarker = import ../workspace-marker.nix;
  workspaceMarkerPresent = workspaceMarker.isPresent projectRoot;
  compiledExecution = (model.compiled or { }).execution or { };
  taskExecutionById = (compiledExecution.tasks or { }).byId or { };
  taskHelpFiles = builtins.mapAttrs (
    taskId: taskExecution:
    pkgs.writeText "nixfied-task-help-${builtins.substring 0 10 (builtins.hashString "sha256" taskId)}.txt" ''
      ${builtins.concatStringsSep "\n" (((taskExecution.help or { }).lines or [ ]))}
    ''
  ) taskExecutionById;
  taskHelpFileFor =
    taskId:
    if builtins.hasAttr taskId taskHelpFiles then
      builtins.toString taskHelpFiles.${taskId}
    else
      throw "dispatcher: missing compiled task help file for '${taskId}'";

  orchestrator = import ./orchestrator.nix {
    inherit
      pkgs
      model
      services
      runtimeHash
      registry
      projectRoot
      serviceSetPrograms
      serviceHookEnv
      executionEnabled
      ;
  };

  orchestratorProgram = "${orchestrator}/bin/nixfied-orchestrator";

  viewApps = model.views.apps;
  viewAppNames = builtins.sort builtins.lessThan (builtins.attrNames viewApps);
  appModels = model.apps or { };
  frameworkInstallHelpFile = taskHelpFileFor "task.framework.install";
  frameworkUpgradeHelpFile = taskHelpFileFor "task.framework.upgrade";
  frameworkSourceFlakeRefValue =
    if frameworkSourceFlakeRef != null && frameworkSourceFlakeRef != "" then
      frameworkSourceFlakeRef
    else
      "github:willyrgf/nixfied/dev";
  frameworkSourceFlakeRefShell = lib.escapeShellArg frameworkSourceFlakeRefValue;
  proxyFrameworkCommand = taskId: helpFile: ''
    if [ "$#" -gt 0 ]; then
      case "$1" in
        --help|-h)
          cat ${helpFile}
          exit 0
          ;;
      esac
    fi

    framework_source_flake_ref="''${NIXFIED_FRAMEWORK_SOURCE_FLAKE:-${frameworkSourceFlakeRefShell}}"
    NIXFIED_CALLER_PWD="$PWD" exec ${pkgs.nix}/bin/nix run "''${framework_source_flake_ref}#run-task" --refresh -- ${lib.escapeShellArg taskId} "$@"
  '';

  viewLaunchApps =
    if !executionEnabled then
      { }
    else
      builtins.listToAttrs (
        map (
          appName:
          let
            appModel = appModels.${appName} or null;
            launchCommand =
              if appModel == null then
                throw "dispatcher: missing app model for '${appName}'"
              else
                ''
                  exec ${appPrograms.${appName}} "$@"
                '';
          in
          {
            name = appName;
            value = mkShellApp {
              inherit appName;
              body = ''
                export NIXFIED_CALLER_PWD="$PWD"
                ${launchCommand}
              '';
            };
          }
        ) viewAppNames
      );

  helpText = builtins.concatStringsSep "\n" model.views.help.lines;
  docsText = builtins.concatStringsSep "\n" model.views.docs.lines;
  featuresText = builtins.concatStringsSep "\n" model.views.features.lines;
  mkStaticHelpFile =
    name: text:
    pkgs.writeText "nixfied-help-${builtins.substring 0 10 (builtins.hashString "sha256" name)}.txt" ''
      ${text}
    '';
  runtimeControlHelpFiles = {
    "runs" = mkStaticHelpFile "runs" ''
      runs - List runs or show one run by id

      Usage:
        nix run .#runs
        nix run .#runs -- <run-id>

      Options:
        -h, --help: Show this help.
    '';

    "stop-run" = mkStaticHelpFile "stop-run" ''
      stop-run - Stop one running or queued run

      Usage:
        nix run .#stop-run -- <run-id>

      Options:
        -h, --help: Show this help.
    '';

    "stop-all-runs" = mkStaticHelpFile "stop-all-runs" ''
      stop-all-runs - Stop all running or queued runs

      Usage:
        nix run .#stop-all-runs

      Options:
        -h, --help: Show this help.
    '';
  };

  helpFile = pkgs.writeText "nixfied-help.txt" "${helpText}\n";
  docsFile = pkgs.writeText "nixfied-docs.md" "${docsText}\n";
  featuresFile = pkgs.writeText "nixfied-features.txt" "${featuresText}\n";

  frameworkProxyApps =
    if workspaceMarkerPresent then
      { }
    else
      {
        "framework::install" = mkShellApp {
          appName = "framework::install";
          body = proxyFrameworkCommand "task.framework.install" frameworkInstallHelpFile;
        };

        "framework::upgrade" = mkShellApp {
          appName = "framework::upgrade";
          body = proxyFrameworkCommand "task.framework.upgrade" frameworkUpgradeHelpFile;
        };
      };
in
{
  "run-task" = mkShellApp {
    appName = "run-task";
    body = ''
      ${shellCommon}
      if [ "$#" -lt 1 ]; then
        nixfied_exit_usage "usage: run-task <task-id> [-- ...]"
      fi
      NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} run-task "$@"
    '';
  };

  "run-workflow" = mkShellApp {
    appName = "run-workflow";
    body = ''
      ${shellCommon}
      if [ "$#" -lt 1 ]; then
        nixfied_exit_usage "usage: run-workflow <workflow-id> [-- ...]"
      fi
      NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} run-workflow "$@"
    '';
  };

  "run-workflow-parallel" = mkShellApp {
    appName = "run-workflow-parallel";
    body = ''
      ${shellCommon}
      if [ "$#" -lt 1 ]; then
        nixfied_exit_usage "usage: run-workflow-parallel <workflow-id> [-- ...]"
      fi
      NIXFIED_WORKFLOW_PARALLEL=1 NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} run-workflow "$@"
    '';
  };

  "help" = mkShellApp {
    appName = "help";
    body = ''
      cat ${helpFile}
    '';
  };

  "docs" = mkShellApp {
    appName = "docs";
    body = ''
      cat ${docsFile}
    '';
  };

  "features" = mkShellApp {
    appName = "features";
    body = ''
      cat ${featuresFile}
    '';
  };
}
// lib.optionalAttrs includeRuntimeControlApps {
  "runs" = mkShellApp {
    appName = "runs";
    body = ''
      ${shellCommon}
      if [ "$#" -eq 1 ]; then
        case "$1" in
          --help|-h)
            cat ${runtimeControlHelpFiles.runs}
            exit 0
            ;;
        esac
      fi
      if [ "$#" -gt 1 ]; then
        nixfied_exit_usage "usage: runs [run-id]"
      fi
      NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} runs "$@"
    '';
  };

  "stop-run" = mkShellApp {
    appName = "stop-run";
    body = ''
      ${shellCommon}
      if [ "$#" -eq 1 ]; then
        case "$1" in
          --help|-h)
            cat ${runtimeControlHelpFiles."stop-run"}
            exit 0
            ;;
        esac
      fi
      if [ "$#" -ne 1 ]; then
        nixfied_exit_usage "usage: stop-run <run-id>"
      fi
      NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} stop-run "$@"
    '';
  };

  "stop-all-runs" = mkShellApp {
    appName = "stop-all-runs";
    body = ''
      ${shellCommon}
      if [ "$#" -eq 1 ]; then
        case "$1" in
          --help|-h)
            cat ${runtimeControlHelpFiles."stop-all-runs"}
            exit 0
            ;;
        esac
      fi
      if [ "$#" -ne 0 ]; then
        nixfied_exit_usage "usage: stop-all-runs"
      fi
      NIXFIED_CALLER_PWD="$PWD" exec ${orchestratorProgram} stop-all-runs
    '';
  };
}
// viewLaunchApps
// frameworkProxyApps
// {
  default =
    if builtins.hasAttr "help" viewLaunchApps then
      viewLaunchApps.help
    else
      mkShellApp {
        appName = "default-help";
        body = ''
          cat ${helpFile}
        '';
      };
}
