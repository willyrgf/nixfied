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
  skipPolicy = import ../runtime/helpers/skip-policy.nix { inherit pkgs; };

  frameworkRoot = ../../.;

  compiled = import ./mkNixfied.nix {
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
  nixpkgsPathAbs = builtins.toString pkgs.path;

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

  serviceNames = builtins.sort builtins.lessThan (
    lib.unique (
      map (
        serviceId:
        let
          service = compiled.services.${serviceId};
        in
        service.name or serviceId
      ) (builtins.attrNames (compiled.services or { }))
    )
  );
  viewAppNames = builtins.sort builtins.lessThan (
    builtins.attrNames (compiled.model.views.apps or { })
  );
  serviceAppNames = builtins.filter (name: lib.hasPrefix "svc::" name) (
    builtins.attrNames (compiled.apps or { })
  );
  dispatcherAppNames = builtins.filter (name: builtins.hasAttr name compiled.apps) [
    "run-task"
    "run-workflow"
    "run-workflow-parallel"
  ];
  nonSelectorAppNames = [
    "framework::install"
    "framework::upgrade"
  ];
  wrappedAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: !(builtins.elem appName nonSelectorAppNames)) (
      lib.unique (viewAppNames ++ serviceAppNames ++ dispatcherAppNames)
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

  taskIds = builtins.sort builtins.lessThan (builtins.attrNames (compiled.model.tasks or { }));

  normalizeTaskArgSpec =
    spec:
    let
      hasLong = (spec ? long) && spec.long != null && spec.long != "";
      hasShort = (spec ? short) && spec.short != null && spec.short != "";
      kind =
        if (spec ? kind) && spec.kind != null then
          spec.kind
        else if hasLong || hasShort then
          "option"
        else
          "positional";
    in
    {
      inherit kind;
      long = if hasLong then spec.long else "";
      short = if hasShort then spec.short else "";
      type = if (spec ? type) && spec.type != null && spec.type != "" then toString spec.type else "";
      values = if (spec ? values) && spec.values != null then map toString spec.values else [ ];
      description = if (spec ? description) && spec.description != null then spec.description else "";
    };

  formatTaskArgHelpLine =
    spec:
    let
      tokens =
        (lib.optionals (spec.short != "") [ spec.short ])
        ++ (lib.optionals (spec.long != "") [ spec.long ]);
      valueLabel =
        if spec.kind != "option" then
          ""
        else if spec.values != [ ] then
          "<${builtins.concatStringsSep "|" spec.values}>"
        else if spec.type != "" then
          "<${spec.type}>"
        else
          "<value>";
      descriptionSuffix = if spec.description != "" then ": ${spec.description}" else "";
    in
    "  ${builtins.concatStringsSep ", " tokens}${
        lib.optionalString (valueLabel != "") " ${valueLabel}"
      }${descriptionSuffix}";

  renderTaskHelpText =
    taskId:
    let
      task = compiled.model.tasks.${taskId};
      app = task.ui.app or { };
      argsContract = (((task.contract or { }).input or { }).args or { });
      specs = map normalizeTaskArgSpec (argsContract.spec or [ ]);
      displayName = if (app.expose or false) && (app.name or "") != "" then app.name else taskId;
      usageLines =
        let
          configuredUsage = app.usage or [ ];
        in
        if configuredUsage != [ ] then configuredUsage else [ "nix run .#run-task -- ${taskId} [-- ...]" ];
      exampleLines = app.examples or [ ];
      optionLines = map formatTaskArgHelpLine specs ++ [
        "  -h, --help: Show this help."
      ];
      summary = task.summary or "";
      description = task.description or "";
    in
    builtins.concatStringsSep "\n" (
      [ "${displayName} - ${summary}" ]
      ++ lib.optionals (description != "") [
        ""
        description
      ]
      ++ [
        ""
        "Usage:"
      ]
      ++ map (line: "  ${line}") usageLines
      ++ [
        ""
        "Options:"
      ]
      ++ optionLines
      ++ lib.optionals (exampleLines != [ ]) (
        [
          ""
          "Examples:"
        ]
        ++ map (line: "  ${line}") exampleLines
      )
    );

  taskHelpFiles = builtins.listToAttrs (
    map (taskId: {
      name = taskId;
      value = pkgs.writeText "nixfied-task-help-${builtins.substring 0 10 (builtins.hashString "sha256" taskId)}.txt" ''
        ${renderTaskHelpText taskId}
      '';
    }) taskIds
  );

  renderTaskHelpCases = builtins.concatStringsSep "\n" (
    map (taskId: ''
      ${lib.escapeShellArg taskId})
        cat ${lib.escapeShellArg (builtins.toString taskHelpFiles.${taskId})}
        return 0
        ;;
    '') taskIds
  );

  mkSelectorAwareLauncher =
    appName:
    let
      internalBaseTarget = internalBaseTargetName appName;
      viewHelpFile =
        if builtins.hasAttr appName (compiled.model.views.apps or { }) then
          builtins.toString taskHelpFiles.${compiled.model.views.apps.${appName}.taskId}
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

                print_fast_task_help() {
                  local task_id="$1"
                  case "$task_id" in
        ${renderTaskHelpCases}
                    *)
                      return 1
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

                for service in "''${known_services[@]}"; do
                  if is_service_skipped "$service"; then
                    requested_excluded_services+=("$service")
                  fi
                done

                excluded_services_csv="$(build_excluded_services_csv)"

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

                if [ -z "$excluded_services_csv" ]; then
                  invocation_pwd="$PWD"
                  flake_root="$(find_flake_root)"
                  cd "$flake_root"
                  NIXFIED_CALLER_PWD="$invocation_pwd" exec ${pkgs.nix}/bin/nix run ".#legacyPackages.${system}._nixfied.baseApps.${internalBaseTarget}" -- "''${forwarded_args[@]}"
                fi

                flake_root="$(find_flake_root)"

        selection_cmd=(
          "${pkgs.nix}/bin/nix-build"
          "--no-out-link"
          "${frameworkRoot}/framework/launch/run-selected-app.nix"
          "--argstr"
          "system"
                  ${lib.escapeShellArg system}
                  "--argstr"
                  "projectRoot"
                  "$flake_root"
                  "--argstr"
                  "frameworkRoot"
                  ${lib.escapeShellArg frameworkRootAbs}
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

  launcherApps =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = appName;
          value = mkSelectorAwareLauncher appName;
        }) wrappedAppNames
      );
  internalBasePackages =
    if !launchersSupported then
      { }
    else
      builtins.listToAttrs (
        map (appName: {
          name = internalBaseTargetName appName;
          value = pkgs.writeShellScriptBin (internalBaseTargetName appName) ''
            set -euo pipefail
            exec ${compiled.apps.${appName}.program} "$@"
          '';
        }) wrappedAppNames
      );
in
compiled
// {
  apps = compiled.apps // launcherApps;
  legacyPackages = {
    _nixfied = {
      baseApps = internalBasePackages;
    };
  };
}
