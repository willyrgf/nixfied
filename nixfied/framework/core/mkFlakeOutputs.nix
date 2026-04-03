{
  pkgs,
  system,
  projectRoot,
  projectModules,
  extraModules ? [ ],
  localOverrides ? [ ],
  frameworkSourceRevision ? import ./framework-revision.nix {
    sourcePath = ../../.;
    metadataPath = ../../VENDORED.txt;
  },
}:
let
  lib = pkgs.lib;
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };
  skipPolicy = import ./skip-policy.nix { inherit pkgs; };
  canonical = import ./canonical.nix { inherit lib; };
  workspaceMarker = import ../workspace-marker.nix;
  registry = import ../runtime/registry { inherit pkgs; };
  kernelPackage = import ../runtime/kernel { inherit pkgs; };

  frameworkRoot = ../../.;
  frameworkRepoRoot = ../../../.;
  frameworkUtilityCommand = import ../install/wrapper-command.nix {
    inherit
      pkgs
      frameworkSourceRevision
      ;
    sourceRoot = frameworkRoot;
    repoRoot = frameworkRepoRoot;
  };

  compiledCore = import ./mkCompiledCore.nix {
    inherit
      pkgs
      system
      projectRoot
      projectModules
      extraModules
      localOverrides
      frameworkSourceRevision
      ;
  };
  validationIrAsset = pkgs.writeText "nixfied-validation-ir.json" ''
    ${builtins.toJSON compiledCore.validationIr}
  '';

  coreSurfaces = import ./mkCoreSurfaces.nix {
    inherit
      pkgs
      canonical
      ;
    compiledCore = compiledCore;
  };

  toAbsString =
    value:
    if builtins.isPath value then
      builtins.toString value
    else if builtins.isString value && lib.hasPrefix "/" value then
      value
    else
      throw "nixfied.mkFlakeOutputs requires path values or absolute path strings for project modules and overrides";

  isPathLike = value: builtins.isPath value || (builtins.isString value && lib.hasPrefix "/" value);

  pathWithin = root: path: path == root || lib.hasPrefix "${root}/" path;

  relativeTo = root: path: if path == root then "" else lib.removePrefix "${root}/" path;

  projectRootAbs = toAbsString projectRoot;
  frameworkRootAbs = builtins.toString frameworkRoot;
  frameworkRootRelSuffix =
    if frameworkRootAbs == projectRootAbs then
      ""
    else if lib.hasPrefix "${projectRootAbs}/" frameworkRootAbs then
      "/${lib.removePrefix "${projectRootAbs}/" frameworkRootAbs}"
    else
      "";
  canRelocateFrameworkRoot =
    frameworkRootAbs == projectRootAbs || lib.hasPrefix "${projectRootAbs}/" frameworkRootAbs;
  nixpkgsPathAbs = builtins.toString pkgs.path;
  launcherRootResolverScript = ''
    can_relocate_framework_root=${lib.escapeShellArg (if canRelocateFrameworkRoot then "1" else "")}
    flake_root="$(find_flake_root)"
    project_root_for_launcher="$flake_root"
    framework_root_for_launcher=${lib.escapeShellArg frameworkRootAbs}
    if [ -n "''${can_relocate_framework_root-}" ]; then
      framework_root_for_launcher="$flake_root${lib.escapeShellArg frameworkRootRelSuffix}"
    fi
  '';

  encodeModuleSpec =
    value:
    let
      absPath = toAbsString value;
    in
    if pathWithin projectRootAbs absPath then
      {
        scope = "project";
        path = relativeTo projectRootAbs absPath;
      }
    else if pathWithin frameworkRootAbs absPath then
      {
        scope = "framework";
        path = relativeTo frameworkRootAbs absPath;
      }
    else
      {
        scope = "absolute";
        path = absPath;
      };

  launchersSupported = builtins.all isPathLike (projectModules ++ extraModules ++ localOverrides);

  projectModuleSpecsJson = builtins.toJSON (map encodeModuleSpec projectModules);
  extraModuleSpecsJson = builtins.toJSON (map encodeModuleSpec extraModules);
  localOverrideSpecsJson = builtins.toJSON (map encodeModuleSpec localOverrides);

  workspaceMarkerPresent = workspaceMarker.isPresent projectRoot;
  compiledExecution = (compiledCore.model.compiled or { }).execution or { };
  serviceSurfaceCatalog = (compiledCore.model.compiled or { }).serviceSurfaceCatalog or { };
  viewAppNames = builtins.sort builtins.lessThan (
    builtins.attrNames (compiledCore.model.views.apps or { })
  );
  selectorDispatcherAppNames = [
    "run-task"
    "run-workflow"
    "run-workflow-parallel"
  ];
  nonSelectorAppNames = [
    "framework::install"
    "framework::upgrade"
  ];
  runtimeProxyAppNames = if workspaceMarkerPresent then [ ] else nonSelectorAppNames;
  serviceNames = compiledExecution.enabledServices or [ ];
  runtimeControlAppNames = [
    "runs"
    "stop-run"
    "stop-all-runs"
  ];
  viewWrappedAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: !(builtins.elem appName nonSelectorAppNames)) (
      lib.unique (viewAppNames ++ selectorDispatcherAppNames)
    )
  );
  serviceWrappedAppNames = builtins.sort builtins.lessThan (serviceSurfaceCatalog.appNames or [ ]);
  runtimeAppNames = builtins.sort builtins.lessThan (
    lib.unique (
      runtimeProxyAppNames
      ++ builtins.filter (appName: builtins.elem appName nonSelectorAppNames) viewAppNames
    )
  );
  internalBaseTargetName =
    appName: "base${builtins.substring 0 10 (builtins.hashString "sha256" appName)}";

  renderKnownServices =
    if serviceNames == [ ] then
      "  (none)"
    else
      builtins.concatStringsSep "\n" (map (serviceName: "  - ${serviceName}") serviceNames);

  renderKnownServicesArray = builtins.concatStringsSep "\n" (
    map (serviceName: "    ${lib.escapeShellArg serviceName}") serviceNames
  );

  taskHelpSupport = import ./mkTaskHelpFiles.nix {
    inherit
      pkgs
      compiledExecution
      ;
  };
  taskHelpFiles = taskHelpSupport.taskHelpFiles;

  mkStaticHelpFile =
    name: text:
    pkgs.writeText "nixfied-help-${builtins.substring 0 10 (builtins.hashString "sha256" name)}.txt" ''
      ${text}
    '';

  dispatcherHelpFiles = {
    "run-task" = mkStaticHelpFile "run-task" ''
      run-task - Run a compiled task by id

      Usage:
        nix run .#run-task -- <task-id> [-- ...]

      Options:
        -h, --help: Show this help.
    '';

    "run-workflow" = mkStaticHelpFile "run-workflow" ''
      run-workflow - Run a compiled workflow by id

      Usage:
        nix run .#run-workflow -- <workflow-id> [-- ...]

      Options:
        -h, --help: Show this help.
    '';

    "run-workflow-parallel" = mkStaticHelpFile "run-workflow-parallel" ''
      run-workflow-parallel - Run a compiled workflow by id with parallel execution enabled

      Usage:
        nix run .#run-workflow-parallel -- <workflow-id> [-- ...]

      Options:
        -h, --help: Show this help.
    '';
  };

  compiledExecutionFile = pkgs.writeText "nixfied-compiled-execution.json" (
    builtins.toJSON compiledExecution
  );
  serviceSetServicesCsvById = builtins.mapAttrs (
    _: serviceSet: builtins.concatStringsSep "," (serviceSet.services.all or [ ])
  ) (compiledCore.serviceSets or { });

  mkSelectorAwareLauncher =
    appName:
    let
      launcherViewApp =
        if builtins.hasAttr appName (compiledCore.model.views.apps or { }) then
          compiledCore.model.views.apps.${appName}
        else
          null;
      launcherTaskId =
        if launcherViewApp != null then
          if (launcherViewApp.taskId or null) == null then "" else launcherViewApp.taskId
        else
          "";
      serviceAppMatch = builtins.match "^svc::([^:]+)::.+$" appName;
      launcherServiceName = if serviceAppMatch == null then "" else builtins.elemAt serviceAppMatch 0;
      launcherServiceSetId =
        if launcherViewApp != null then
          if (launcherViewApp.serviceSetId or null) == null then "" else launcherViewApp.serviceSetId
        else
          "";
      viewHelpFile =
        if
          launcherViewApp != null && (launcherViewApp.taskId or null) != null && launcherViewApp.taskId != ""
        then
          builtins.toString taskHelpFiles.${launcherViewApp.taskId}
        else
          "";
      dispatcherHelpFile =
        if builtins.hasAttr appName dispatcherHelpFiles then
          builtins.toString dispatcherHelpFiles.${appName}
        else
          "";
    in
    mkShellApp {
      inherit appName;
      binPrefix = "nixfied-launch";
      body = ''
                ${skipPolicy.skipPolicyFunctions}

                known_services=(
                ${renderKnownServicesArray}
                )

                render_launcher_help() {
                  cat <<'NIXFIED_LAUNCHER_HELP'
        ${appName} accepts framework launcher selectors before normal app arguments.

        Launcher options:
          --exclude-services <csv>: Exclude services from the compiled graph.
          --launcher-help: Show this help.

        Selector parsing stops at the first non-launcher argument or `--`, and the
        remaining arguments are forwarded unchanged to the selected app.

        Compatibility:
          Truthy SKIP_<SERVICE> env vars are folded into the excluded set as
          launcher sugar. The explicit --exclude-services selector is the
          canonical compile-time interface.

        Known services:
        ${renderKnownServices}
        NIXFIED_LAUNCHER_HELP
                }

                service_is_known() {
                  local target="$1"
                  local service=""
                  for service in "''${known_services[@]}"; do
                    if [ "$service" = "$target" ]; then
                      return 0
                    fi
                  done
                  return 1
                }

                append_excluded_service() {
                  local service="$1"
                  if [ -z "$service" ]; then
                    return 0
                  fi

                  if ! service_is_known "$service"; then
                    echo "ERROR: unknown service '$service' in --exclude-services" >&2
                    exit 2
                  fi

                  requested_excluded_services+=("$service")
                }

                parse_excluded_services_csv() {
                  local csv="$1"
                  local token=""
                  local old_ifs="$IFS"
                  local csv_parts=()

                  IFS=','
                  read -r -a csv_parts <<< "$csv"
                  IFS="$old_ifs"

                  for token in "''${csv_parts[@]}"; do
                    token="$(printf '%s' "$token" | ${pkgs.coreutils}/bin/tr -d '[:space:]')"
                    [ -n "$token" ] || continue
                    append_excluded_service "$token"
                  done
                }

                build_excluded_services_csv() {
                  if [ "''${#requested_excluded_services[@]}" -eq 0 ]; then
                    printf '%s' ""
                    return 0
                  fi

                  printf '%s\n' "''${requested_excluded_services[@]}" \
                    | ${pkgs.coreutils}/bin/sort -u \
                    | ${pkgs.coreutils}/bin/paste -sd, -
                }

                find_flake_root() {
                  local dir="''${NIXFIED_FLAKE_ROOT:-''${NIXFIED_CALLER_PWD:-$PWD}}"
                  while [ "$dir" != "/" ]; do
                    if [ -f "$dir/flake.nix" ]; then
                      printf '%s' "$dir"
                      return 0
                    fi
                    dir="$(${pkgs.coreutils}/bin/dirname "$dir")"
                  done

                  echo "ERROR: unable to locate flake root from ''${NIXFIED_FLAKE_ROOT:-''${NIXFIED_CALLER_PWD:-$PWD}}" >&2
                  exit 3
                }

                forwarded_args_request_help() {
                  local arg=""
                  while [ "$#" -gt 0 ]; do
                    arg="$1"
                    shift

                    case "$arg" in
                      --help|-h)
                        return 0
                        ;;
                      --)
                        return 1
                        ;;
                    esac
                  done

                  return 1
                }

                forwarded_args_only_help_flag() {
                  if [ "$#" -ne 1 ]; then
                    return 1
                  fi

                  case "$1" in
                    --help|-h)
                      return 0
                      ;;
                    *)
                      return 1
                      ;;
                  esac
                }

                print_fast_task_help() {
                  local task_id="$1"
                  case "$task_id" in
        ${taskHelpSupport.renderTaskHelpCases}
                    *)
                      return 1
                      ;;
                  esac
                }

                launcher_task_selected_services_csv() {
                  local task_id="$1"
                  shift

                  if ! ${kernelPackage}/bin/nixfied-kernel task selected-services-csv ${lib.escapeShellArg (builtins.toString compiledExecutionFile)} "$task_id" -- "$@" 2>/dev/null; then
                    printf '%s' ""
                  fi
                }

                launcher_workflow_selected_services_csv() {
                  local workflow_id="$1"
                  shift

                  if ! ${kernelPackage}/bin/nixfied-kernel workflow selected-services-csv ${lib.escapeShellArg (builtins.toString compiledExecutionFile)} "$workflow_id" -- "$@" 2>/dev/null; then
                    printf '%s' ""
                  fi
                }

                launcher_selected_services_csv() {
                  local task_id=""
                  local workflow_id=""

                  if [ -n ${lib.escapeShellArg launcherTaskId} ]; then
                    launcher_task_selected_services_csv ${lib.escapeShellArg launcherTaskId} "$@"
                    return 0
                  fi

                  if [ -n ${lib.escapeShellArg launcherServiceName} ]; then
                    printf '%s' ${lib.escapeShellArg launcherServiceName}
                    return 0
                  fi

                  if [ -n ${lib.escapeShellArg launcherServiceSetId} ]; then
                    printf '%s' ${
                      lib.escapeShellArg (serviceSetServicesCsvById.${launcherServiceSetId} or "")
                    }
                    return 0
                  fi

                  case ${lib.escapeShellArg appName} in
                    run-task)
                      task_id="''${1:-}"
                      if [ -z "$task_id" ]; then
                        printf '%s' ""
                        return 0
                      fi
                      shift
                      launcher_task_selected_services_csv "$task_id" "$@"
                      ;;
                    run-workflow|run-workflow-parallel)
                      workflow_id="''${1:-}"
                      if [ -z "$workflow_id" ]; then
                        printf '%s' ""
                        return 0
                      fi
                      shift
                      launcher_workflow_selected_services_csv "$workflow_id" "$@"
                      ;;
                    *)
                      printf '%s' ""
                      ;;
                  esac
                }

                requested_excluded_services=()
                forwarded_args=()

                while [ "$#" -gt 0 ]; do
                  case "$1" in
                    --launcher-help)
                      render_launcher_help
                      exit 0
                      ;;
                    --exclude-services)
                      if [ "$#" -lt 2 ]; then
                        echo "ERROR: option '--exclude-services' requires a value" >&2
                        exit 2
                      fi
                      parse_excluded_services_csv "$2"
                      shift 2
                      ;;
                    --exclude-services=*)
                      parse_excluded_services_csv "''${1#--exclude-services=}"
                      shift
                      ;;
                    --)
                      shift
                      while [ "$#" -gt 0 ]; do
                        forwarded_args+=("$1")
                        shift
                      done
                      break
                      ;;
                    *)
                      while [ "$#" -gt 0 ]; do
                        forwarded_args+=("$1")
                        shift
                      done
                      break
                      ;;
                  esac
                done

                if [ -n ${lib.escapeShellArg dispatcherHelpFile} ] \
                  && forwarded_args_only_help_flag "''${forwarded_args[@]}"; then
                  cat ${lib.escapeShellArg dispatcherHelpFile}
                  exit 0
                fi

                if forwarded_args_request_help "''${forwarded_args[@]}"; then
                  if [ -n ${lib.escapeShellArg viewHelpFile} ]; then
                    cat ${lib.escapeShellArg viewHelpFile}
                    exit 0
                  fi

                  if [ ${lib.escapeShellArg appName} = 'run-task' ] \
                    && print_fast_task_help "''${forwarded_args[0]:-}"; then
                    exit 0
                  fi
                fi

                for service in "''${known_services[@]}"; do
                  if is_service_skipped "$service"; then
                    requested_excluded_services+=("$service")
                  fi
                done

                excluded_services_csv="$(build_excluded_services_csv)"

                selected_services_csv="$(launcher_selected_services_csv "''${forwarded_args[@]}")"
                if [ -n "$excluded_services_csv" ] && [ -z "$selected_services_csv" ]; then
                  selected_services_csv="__ALL__"
                fi

                ${launcherRootResolverScript}

        selection_cmd=(
          "${pkgs.nix}/bin/nix-build"
          "--no-out-link"
          "$framework_root_for_launcher/framework/launch/run-selected-app.nix"
          "--argstr"
                  "system"
                  ${lib.escapeShellArg system}
                  "--argstr"
                  "projectRoot"
                  "$project_root_for_launcher"
                  "--argstr"
                  "frameworkRoot"
                  "$framework_root_for_launcher"
                  "--argstr"
                  "nixpkgsPath"
                  ${lib.escapeShellArg nixpkgsPathAbs}
                  "--argstr"
                  "frameworkSourceRevision"
                  ${lib.escapeShellArg frameworkSourceRevision}
                  "--argstr"
                  "appName"
                  ${lib.escapeShellArg appName}
                  "--argstr"
                  "excludedServicesCsv"
                  "$excluded_services_csv"
                  "--argstr"
                  "selectedServicesCsv"
                  "$selected_services_csv"
                  "--argstr"
                  "projectModuleSpecsJson"
                  ${lib.escapeShellArg projectModuleSpecsJson}
                  "--argstr"
                  "extraModuleSpecsJson"
                  ${lib.escapeShellArg extraModuleSpecsJson}
          "--argstr"
          "localOverrideSpecsJson"
          ${lib.escapeShellArg localOverrideSpecsJson}
        )

        selected_launcher="$("''${selection_cmd[@]}")"
        shopt -s nullglob
        selected_programs=("$selected_launcher"/bin/*)
        shopt -u nullglob

        if [ "''${#selected_programs[@]}" -ne 1 ]; then
          echo "ERROR: expected exactly one selected launcher binary for app ${appName}" >&2
          exit 3
        fi

        exec "''${selected_programs[0]}" "''${forwarded_args[@]}"
      '';
    };

  mkRuntimeAppLauncher =
    appName:
    mkShellApp {
      inherit appName;
      binPrefix = "nixfied-runtime-launch";
      body = ''
        find_flake_root() {
          local dir="''${NIXFIED_FLAKE_ROOT:-''${NIXFIED_CALLER_PWD:-$PWD}}"
          while [ "$dir" != "/" ]; do
            if [ -f "$dir/flake.nix" ]; then
              printf '%s' "$dir"
              return 0
            fi
            dir="$(${pkgs.coreutils}/bin/dirname "$dir")"
          done

          echo "ERROR: unable to locate flake root from ''${NIXFIED_FLAKE_ROOT:-''${NIXFIED_CALLER_PWD:-$PWD}}" >&2
          exit 3
        }

        ${launcherRootResolverScript}

        runtime_cmd=(
          "${pkgs.nix}/bin/nix-build"
          "--no-out-link"
          "$framework_root_for_launcher/framework/launch/run-runtime-app.nix"
          "--argstr"
          "system"
          ${lib.escapeShellArg system}
          "--argstr"
          "projectRoot"
          "$project_root_for_launcher"
          "--argstr"
          "frameworkRoot"
          "$framework_root_for_launcher"
          "--argstr"
          "nixpkgsPath"
          ${lib.escapeShellArg nixpkgsPathAbs}
          "--argstr"
          "frameworkSourceRevision"
          ${lib.escapeShellArg frameworkSourceRevision}
          "--argstr"
          "appName"
          ${lib.escapeShellArg appName}
          "--argstr"
          "projectModuleSpecsJson"
          ${lib.escapeShellArg projectModuleSpecsJson}
          "--argstr"
          "extraModuleSpecsJson"
          ${lib.escapeShellArg extraModuleSpecsJson}
          "--argstr"
          "localOverrideSpecsJson"
          ${lib.escapeShellArg localOverrideSpecsJson}
        )

        selected_launcher="$("''${runtime_cmd[@]}")"
        shopt -s nullglob
        selected_programs=("$selected_launcher"/bin/*)
        shopt -u nullglob

        if [ "''${#selected_programs[@]}" -ne 1 ]; then
          echo "ERROR: expected exactly one selected launcher binary for app ${appName}" >&2
          exit 3
        fi

        exec "''${selected_programs[0]}" "$@"
      '';
    };

  viewSelectorLauncherApps =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = appName;
          value = mkSelectorAwareLauncher appName;
        }) viewWrappedAppNames
      );
  serviceSelectorLauncherApps =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = appName;
          value = mkSelectorAwareLauncher appName;
        }) serviceWrappedAppNames
      );
  runtimeLauncherApps =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = appName;
          value = mkRuntimeAppLauncher appName;
        }) runtimeAppNames
      );
  frameworkUtilityApps = {
    "framework::install" = mkShellApp {
      appName = "framework::install";
      binPrefix = "nixfied-framework";
      body = ''
        if [ "$#" -gt 0 ]; then
          case "$1" in
            --help|-h)
              cat ${lib.escapeShellArg (taskHelpSupport.taskHelpFileFor "task.framework.install")}
              exit 0
              ;;
          esac
        fi

        ${frameworkUtilityCommand { }}
      '';
    };

    "framework::upgrade" = mkShellApp {
      appName = "framework::upgrade";
      binPrefix = "nixfied-framework";
      body = ''
        if [ "$#" -gt 0 ]; then
          case "$1" in
            --help|-h)
              cat ${lib.escapeShellArg (taskHelpSupport.taskHelpFileFor "task.framework.upgrade")}
              exit 0
              ;;
          esac
        fi

        ${frameworkUtilityCommand {
          upgradeDefault = true;
        }}
      '';
    };
  };
  controlOnlyDispatcherApps =
    let
      controlDispatcher = import ../runtime/dispatcher.nix {
        inherit
          pkgs
          projectRoot
          registry
          ;
        model = compiledCore.model;
        services = { };
        appPrograms = { };
        serviceSetPrograms = { };
        serviceHookEnv = { };
        executionEnabled = false;
      };
    in
    builtins.listToAttrs (
      map (appName: {
        name = appName;
        value = controlDispatcher.${appName};
      }) runtimeControlAppNames
    );
  heavyOutputs =
    let
      materializedExecution = import ./materializeExecution.nix {
        inherit
          pkgs
          projectRoot
          ;
        compiledCore = compiledCore;
        frameworkSourceFlakeRef = null;
      };
    in
    {
      runtimeHash = materializedExecution.runtimeHash or compiledCore.model.identity.evalHash;
      services = materializedExecution.services;
      serviceApis = materializedExecution.serviceApis;
      serviceHookEnv = materializedExecution.serviceHookEnv;
      directApps =
        materializedExecution.baseApps
        // coreSurfaces.apps
        // {
          default =
            if builtins.hasAttr "help" coreSurfaces.apps then
              coreSurfaces.apps.help
            else if builtins.hasAttr "default" materializedExecution.baseApps then
              materializedExecution.baseApps.default
            else
              materializedExecution.baseApps.help;
        };
      frameworkWorkspaceApps =
        if
          workspaceMarkerPresent && builtins.hasAttr "framework::test" (compiledCore.model.views.apps or { })
        then
          {
            "framework::test" = mkShellApp {
              appName = "framework::test";
              binPrefix = "nixfied-framework";
              body = ''
                if [ "$#" -gt 0 ]; then
                  case "$1" in
                    --help|-h)
                      cat ${lib.escapeShellArg (taskHelpSupport.taskHelpFileFor "task.framework.test")}
                      exit 0
                      ;;
                  esac
                fi

                cd ${lib.escapeShellArg projectRootAbs}
                exec ${lib.escapeShellArg materializedExecution.baseApps."framework::test".program} "$@"
              '';
            };
          }
        else
          { };
    };
  internalBasePackages =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = internalBaseTargetName appName;
          value = pkgs.writeShellScriptBin (internalBaseTargetName appName) ''
            set -euo pipefail
              exec ${
                if builtins.elem appName viewWrappedAppNames then
                  viewSelectorLauncherApps.${appName}.program
                else
                  serviceSelectorLauncherApps.${appName}.program
              } "$@"
          '';
        }) (viewWrappedAppNames ++ serviceWrappedAppNames)
      );
in
{
  model = compiledCore.model;
  stateHash = compiledCore.stateHash;
  runtimeHash = heavyOutputs.runtimeHash;
  tasks = compiledCore.model.tasks;
  services = heavyOutputs.services;
  serviceDefinitions = compiledCore.resolved.services or { };
  serviceCatalog = compiledCore.model.serviceCatalog;
  workflows = compiledCore.model.workflows;
  features = compiledCore.model.features;
  serviceSurfaceCatalog = (compiledCore.model.compiled or { }).serviceSurfaceCatalog or { };
  serviceApis = heavyOutputs.serviceApis;
  serviceHookEnv = heavyOutputs.serviceHookEnv;
  packages = coreSurfaces.packages // {
    default = pkgs.runCommand "nixfied-default" { } ''
      mkdir -p "$out/bin"
      ln -s ${
        if launchersSupported then
          coreSurfaces.apps.help.program
        else
          heavyOutputs.directApps.default.program
      } "$out/bin/default"
    '';
    "nixfied-kernel" = kernelPackage;
    "validation-ir" = validationIrAsset;
  };
  validationIr = compiledCore.validationIr;
  checks = coreSurfaces.checks;
  devShells = coreSurfaces.devShells;
  schema = coreSurfaces.schema;
  apps =
    if launchersSupported then
      serviceSelectorLauncherApps
      // coreSurfaces.apps
      // viewSelectorLauncherApps
      // runtimeLauncherApps
      // controlOnlyDispatcherApps
      // frameworkUtilityApps
      // heavyOutputs.frameworkWorkspaceApps
      // {
        default = coreSurfaces.apps.help;
      }
    else
      heavyOutputs.directApps
      // controlOnlyDispatcherApps
      // frameworkUtilityApps
      // heavyOutputs.frameworkWorkspaceApps;
  legacyPackages = {
    _nixfied = {
      baseApps = internalBasePackages;
    };
  };
}
