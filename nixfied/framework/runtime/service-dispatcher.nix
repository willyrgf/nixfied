{
  pkgs,
  serviceSurfaceCatalog,
  serviceOperationPrograms,
}:
let
  lib = pkgs.lib;
  skipPolicy = import ../core/skip-policy.nix { inherit pkgs; };

  operationEntries = builtins.concatLists (
    map
      (
        serviceName:
        let
          ops = serviceSurfaceCatalog.operationCatalog.${serviceName} or { };
        in
        map
          (
            opName:
            let
              op = ops.${opName};
              appName = op.appName or "";
            in
            {
              key = "${serviceName}:${opName}";
              inherit
                appName
                serviceName
                opName
                ;
              program = serviceOperationPrograms.${appName} or "";
            }
          )
          (
            builtins.filter (
              opName:
              let
                op = ops.${opName};
                appName = op.appName or "";
              in
              appName != "" && builtins.hasAttr appName serviceOperationPrograms
            ) (builtins.sort builtins.lessThan (builtins.attrNames ops))
          )
      )
      (
        builtins.sort builtins.lessThan (builtins.attrNames (serviceSurfaceCatalog.operationCatalog or { }))
      )
  );
in
pkgs.writeShellScriptBin "nixfied-service-dispatcher" ''
    set -euo pipefail
    ${skipPolicy.skipPolicyFunctions}

    service_program_for() {
      local service_name="$1"
      local op_name="$2"

      case "$service_name:$op_name" in
  ${builtins.concatStringsSep "\n" (
    map (entry: ''
      ${lib.escapeShellArg entry.key})
        printf '%s' ${lib.escapeShellArg entry.program}
        return 0
        ;;
    '') operationEntries
  )}
        *)
          return 1
          ;;
      esac
    }

    has_service_op() {
      local service_name="$1"
      local op_name="$2"

      service_program_for "$service_name" "$op_name" >/dev/null 2>&1
    }

    run_service() {
      if [ "$#" -lt 2 ]; then
        echo "ERROR: usage: run-service <service> <op> [-- ...]" >&2
        return 2
      fi

      local service_name="$1"
      local op_name="$2"
      shift 2
      local program_path=""
      local parse_options=1
      local arg=""
      local -a passthrough_args=()

      while [ "$#" -gt 0 ]; do
        arg="$1"
        shift

        if [ "$parse_options" -eq 0 ]; then
          passthrough_args+=("$arg")
          continue
        fi

        case "$arg" in
          --exclude-services)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --exclude-services requires a value" >&2
              return 2
            fi
            export NIXFIED_EXCLUDED_SERVICES_CSV="$1"
            shift
            ;;
          --exclude-services=*)
            export NIXFIED_EXCLUDED_SERVICES_CSV="''${arg#--exclude-services=}"
            ;;
          --)
            parse_options=0
            passthrough_args+=("--")
            ;;
          *)
            passthrough_args+=("$arg")
            ;;
        esac
      done

      if excluded_services_contains "$service_name"; then
        echo "ERROR: service '$service_name' is excluded by --exclude-services" >&2
        return 2
      fi

      program_path="$(service_program_for "$service_name" "$op_name" || true)"
      if [ -z "$program_path" ]; then
        echo "ERROR: unknown service operation service=$service_name op=$op_name" >&2
        return 2
      fi

      exec "$program_path" "''${passthrough_args[@]}"
    }

    main() {
      if [ "$#" -lt 1 ]; then
        echo "ERROR: usage: nixfied-service-dispatcher <run-service|has-service-op> ..." >&2
        exit 2
      fi

      local subcommand="$1"
      shift

      case "$subcommand" in
        run-service)
          run_service "$@"
          ;;
        has-service-op)
          if [ "$#" -ne 2 ]; then
            echo "ERROR: usage: has-service-op <service> <op>" >&2
            exit 2
          fi
          if has_service_op "$1" "$2"; then
            exit 0
          fi
          exit 1
          ;;
        *)
          echo "ERROR: unknown subcommand '$subcommand'" >&2
          exit 2
          ;;
      esac
    }

    main "$@"
''
