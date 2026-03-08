{
  lib,
  config,
  pkgs,
  ...
}:
let
  cfg = config.nixfied.operations;
  runtime = config.nixfied.runtime;
  services = config.nixfied.services;
  probeCommands = import ../.framework/lib/probe-commands.nix { inherit pkgs; };
  serviceConfigLib = import ../lib/service-config.nix { inherit lib; };

  postgresCfg = services.postgres;
  nginxCfg = services.nginx;
  minioCfg = services.minio;
  rethCfg = services.reth;
  heliosCfg = services.helios;

  postgresEnabled = postgresCfg.enable;
  nginxEnabled = nginxCfg.enable;
  minioEnabled = minioCfg.enable;
  rethEnabled = rethCfg.enable;
  heliosEnabled = heliosCfg.enable;

  serviceNames = [
    "postgres"
    "nginx"
    "minio"
    "reth"
    "helios"
  ];

  serviceEnabledByName = {
    postgres = postgresEnabled;
    nginx = nginxEnabled;
    minio = minioEnabled;
    reth = rethEnabled;
    helios = heliosEnabled;
  };

  serviceConfigByName = {
    postgres = postgresCfg;
    nginx = nginxCfg;
    minio = minioCfg;
    reth = rethCfg;
    helios = heliosCfg;
  };

  resolvedServiceConfigByName = builtins.mapAttrs (
    serviceName: serviceCfg:
    serviceConfigLib.normalizeServiceConfig {
      name = serviceName;
      config = serviceCfg;
    }
  ) serviceConfigByName;

  enabledServiceNames = builtins.filter (
    serviceName: serviceEnabledByName.${serviceName}
  ) serviceNames;

  resolvePortBase =
    key:
    if builtins.hasAttr key runtime.ports then
      runtime.ports.${key}
    else
      throw "nixfied.operations: port key '${key}' is not defined in nixfied.runtime.ports";

  resolveServicePortBase =
    serviceName: endpointName:
    resolvePortBase
      resolvedServiceConfigByName.${serviceName}.resolved.endpoints.${endpointName}.portKey;

  postgresPortBase = if postgresEnabled then resolveServicePortBase "postgres" "primary" else 0;
  nginxHttpPortBase = if nginxEnabled then resolveServicePortBase "nginx" "http" else 0;
  nginxHttpsPortBase = if nginxEnabled then resolveServicePortBase "nginx" "https" else 0;
  minioApiPortBase = if minioEnabled then resolveServicePortBase "minio" "api" else 0;
  minioConsolePortBase = if minioEnabled then resolveServicePortBase "minio" "console" else 0;
  rethHttpPortBase = if rethEnabled then resolveServicePortBase "reth" "http" else 0;
  rethWsPortBase = if rethEnabled then resolveServicePortBase "reth" "ws" else 0;
  rethAuthPortBase = if rethEnabled then resolveServicePortBase "reth" "auth" else 0;
  heliosExecutionRpcPortBase =
    if heliosEnabled then resolveServicePortBase "helios" "execution" else 0;
  heliosRpcPortBase = if heliosEnabled then resolveServicePortBase "helios" "rpc" else 0;

  netcatPkg =
    if pkgs ? netcat then
      pkgs.netcat
    else if pkgs ? netcat-openbsd then
      pkgs.netcat-openbsd
    else
      throw "nixfied.operations: netcat package is required for readiness probes";

  postgresProbePkg = if pkgs ? postgresql_16 then pkgs.postgresql_16 else pkgs.postgresql;
  heliosSourceKinds = heliosCfg.sourceKinds or { };
  heliosSourceKindCase = builtins.concatStringsSep "\n" (
    map (
      sourceName:
      "      ${lib.escapeShellArg sourceName}) printf '%s' ${
              lib.escapeShellArg (heliosSourceKinds.${sourceName} or "unknown")
            } ;;"
    ) (builtins.sort builtins.lessThan (builtins.attrNames heliosSourceKinds))
  );
  heliosReadinessProfile = heliosCfg.readiness.profile or "fast";
  heliosReadinessRequireNotSyncing =
    (heliosCfg.readiness.requireNotSyncing or false) || heliosReadinessProfile == "strict";
  heliosReadinessDisallowSourceKinds = lib.unique (
    (heliosCfg.readiness.disallowSourceKinds or [ ])
    ++ lib.optionals (heliosReadinessProfile == "strict") [
      "shim"
      "unknown"
    ]
  );
  heliosReadinessDisallowSourceKindArgs = builtins.concatStringsSep " " (
    map lib.escapeShellArg heliosReadinessDisallowSourceKinds
  );
  serviceProbeRuntimeInputs = [
    pkgs.coreutils
    pkgs.gnugrep
    pkgs.gnused
    pkgs.curl
    pkgs.jq
    netcatPkg
    postgresProbePkg
  ];

  envNames = runtime.env.names;
  envPattern =
    if envNames == [ ] then runtime.env.default else builtins.concatStringsSep "|" envNames;
  isolationEnvValues = if envNames == [ ] then [ runtime.env.default ] else envNames;
  isolationSlotVar = runtime.slot.var;
  isolationEnvVar = runtime.env.var;
  isolationMaxSlot = runtime.slot.max;
  isolationSlotsJson = builtins.toJSON cfg.testIsolation.slots;
  isolationEnvsJson = builtins.toJSON cfg.testIsolation.envs;
  isolationRunTaskId = cfg.testIsolation.runTaskId;
  isolationValidateTaskId = cfg.testIsolation.validateTaskId;
  isolationRunArgsJson = builtins.toJSON cfg.testIsolation.runArgs;
  isolationRunEnvJson = builtins.toJSON cfg.testIsolation.runEnv;

  envOffsetCase = builtins.concatStringsSep "\n" (
    map (
      envName: "    ${envName}) env_offset=${toString (runtime.env.offsets.${envName} or 0)} ;;"
    ) envNames
  );

  slotEnvPrelude = ''
        slot_var=${lib.escapeShellArg runtime.slot.var}
        env_var=${lib.escapeShellArg runtime.env.var}
        slot_default=${toString runtime.slot.default}
        env_default=${lib.escapeShellArg runtime.env.default}

        slot_value="''${!slot_var:-$slot_default}"
        env_value="''${!env_var:-$env_default}"

        if ! [[ "$slot_value" =~ ^[0-9]+$ ]]; then
          echo "ERROR: $slot_var must be an integer"
          exit 3
        fi

        case "$env_value" in
    ${envOffsetCase}
          *)
            echo "ERROR: unsupported $env_var '$env_value'"
            exit 3
            ;;
        esac
  '';

  portNames = builtins.sort builtins.lessThan (builtins.attrNames runtime.ports);
  portEmitLines = builtins.concatStringsSep "\n" (
    map (
      portName:
      let
        base = runtime.ports.${portName};
      in
      ''
        value=$(( ${toString base} + env_offset + (slot_value * ${toString runtime.slot.stride}) ))
        printf "%s=%s\n" "${lib.toUpper portName}_PORT" "$value"
      ''
    ) portNames
  );

  portCheckLines = builtins.concatStringsSep "\n" (
    map (
      portName:
      let
        base = runtime.ports.${portName};
      in
      ''
        value=$(( ${toString base} + env_offset + (slot_value * ${toString runtime.slot.stride}) ))
        if command -v lsof >/dev/null 2>&1; then
          if lsof -nP -iTCP:"$value" -sTCP:LISTEN >/dev/null 2>&1; then
            status="LISTEN"
          else
            status="FREE"
          fi
        else
          status="UNKNOWN"
        fi
        printf "%s=%s (%s)\n" "${lib.toUpper portName}_PORT" "$value" "$status"
      ''
    ) portNames
  );

  serviceSelectionContractArgs = [
    {
      name = "service";
      kind = "option";
      long = "--service";
      type = "enum";
      values = serviceNames ++ [ "all" ];
      description = "Select one enabled service or 'all' (default).";
    }
    {
      name = "source";
      kind = "option";
      long = "--source";
      type = "string";
      description = "Override source key for selected service (requires --service).";
    }
  ];

  testIsolationContractArgs = [
    {
      name = "slot";
      kind = "option";
      long = "--slot";
      type = "int";
      description = "Run a single isolation slot (requires --env).";
    }
    {
      name = "env";
      kind = "option";
      long = "--env";
      type = "enum";
      values = isolationEnvValues;
      description = "Run a single isolation environment (requires --slot).";
    }
    {
      name = "max-parallel";
      kind = "option";
      long = "--max-parallel";
      type = "int";
      min = 1;
      description = "Override the isolation worker cap for this invocation.";
    }
  ];

  knownServiceCase = builtins.concatStringsSep "\n" (
    map (serviceName: "      ${serviceName}) return 0 ;;") serviceNames
  );

  serviceEnabledCase = builtins.concatStringsSep "\n" (
    map (
      serviceName:
      "      ${serviceName}) echo ${if serviceEnabledByName.${serviceName} then "1" else "0"} ;;"
    ) serviceNames
  );

  serviceDefaultSourceCase = builtins.concatStringsSep "\n" (
    map (
      serviceName:
      "      ${serviceName}) printf '%s' ${
              lib.escapeShellArg (resolvedServiceConfigByName.${serviceName}.defaultSource or "")
            } ;;"
    ) serviceNames
  );

  serviceHasSourceCase = builtins.concatStringsSep "\n" (
    map (
      serviceName:
      let
        sourceKeys = resolvedServiceConfigByName.${serviceName}.sourceKeys or [ ];
        sourceArgs = builtins.concatStringsSep " " (map lib.escapeShellArg sourceKeys);
      in
      ''
        ${serviceName})
          source_key_matches "$source"${if sourceArgs == "" then "" else " ${sourceArgs}"}
          return $?
          ;;
      ''
    ) serviceNames
  );

  enabledServiceArrayInit =
    if enabledServiceNames == [ ] then
      "selected_services=()"
    else
      "selected_services=("
      + builtins.concatStringsSep " " (map lib.escapeShellArg enabledServiceNames)
      + ")";

  serviceSelectionPrelude = ''
        target_service="all"
        target_source=""

        while [ "$#" -gt 0 ]; do
          case "$1" in
            --service)
              if [ "$#" -lt 2 ]; then
                echo "ERROR: --service requires a value"
                exit 2
              fi
              target_service="$2"
              shift 2
              ;;
            --service=*)
              target_service="''${1#--service=}"
              shift
              ;;
            --source)
              if [ "$#" -lt 2 ]; then
                echo "ERROR: --source requires a value"
                exit 2
              fi
              target_source="$2"
              shift 2
              ;;
            --source=*)
              target_source="''${1#--source=}"
              shift
              ;;
            --)
              shift
              break
              ;;
            *)
              echo "ERROR: unknown argument '$1'"
              exit 2
              ;;
          esac
        done

        if [ "$#" -gt 0 ]; then
          echo "ERROR: unexpected positional arguments: $*"
          exit 2
        fi

        is_known_service() {
          case "$1" in
    ${knownServiceCase}
            *) return 1 ;;
          esac
        }

        is_service_enabled() {
          case "$1" in
    ${serviceEnabledCase}
            *) echo "0" ;;
          esac
        }

        service_default_source() {
          case "$1" in
    ${serviceDefaultSourceCase}
            *) printf '%s' "" ;;
          esac
        }

        source_key_matches() {
          local wanted="$1"
          shift
          local candidate
          for candidate in "$@"; do
            if [ "$candidate" = "$wanted" ]; then
              return 0
            fi
          done
          return 1
        }

        source_kind_disallowed() {
          local source_kind="$1"
          shift
          local blocked_kind
          for blocked_kind in "$@"; do
            if [ "$blocked_kind" = "$source_kind" ]; then
              return 0
            fi
          done
          return 1
        }

        helios_source_kind() {
          local source="$1"
          case "$source" in
    ${heliosSourceKindCase}
            *)
              printf '%s' "unknown"
              ;;
          esac
        }

        service_has_source() {
          local service="$1"
          local source="$2"
          case "$service" in
    ${serviceHasSourceCase}
            *)
              return 1
              ;;
          esac
        }

        service_selected() {
          local service="$1"
          local selected
          for selected in "''${selected_services[@]}"; do
            if [ "$selected" = "$service" ]; then
              return 0
            fi
          done
          return 1
        }

        resolve_service_source() {
          local service="$1"
          if [ -n "$target_source" ]; then
            printf '%s' "$target_source"
            return 0
          fi
          service_default_source "$service"
        }

        if [ "$target_service" != "all" ] && ! is_known_service "$target_service"; then
          echo "ERROR: unknown --service '$target_service'"
          exit 2
        fi

        if [ "$target_service" = "all" ]; then
          ${enabledServiceArrayInit}
        else
          if [ "$(is_service_enabled "$target_service")" != "1" ]; then
            echo "ERROR: selected service '$target_service' is disabled"
            exit 3
          fi
          selected_services=("$target_service")
        fi

        if [ -n "$target_source" ]; then
          if [ "$target_service" = "all" ]; then
            echo "ERROR: --source requires --service"
            exit 2
          fi
          if ! service_has_source "$target_service" "$target_source"; then
            echo "ERROR: unknown source '$target_source' for service '$target_service'"
            exit 3
          fi
        fi

  '';

  mkServiceProbeSection =
    mode: serviceName: spec:
    let
      modeLabel = if mode == "health" then "health" else "readiness";
      skipMessage = spec.skipMessage or "SKIP: ${serviceName} ${modeLabel} check not selected";
    in
    ''
      if service_selected "${serviceName}"; then
        service_source="$(resolve_service_source "${serviceName}")"
        if [ -z "$service_source" ]; then
          service_source="unspecified"
        fi
        checks=$((checks + ${toString spec.count}))
        ${spec.body}
      else
        echo ${lib.escapeShellArg skipMessage}
      fi
    '';

  mkProbeScript =
    {
      mode,
      serviceSpecs,
      emptyMessage,
      successMessage,
    }:
    ''
      set -euo pipefail
      ${slotEnvPrelude}
      ${serviceSelectionPrelude}

      if [ "$target_service" = "all" ] && [ ${toString (builtins.length enabledServiceNames)} -eq 0 ]; then
        echo ${lib.escapeShellArg emptyMessage}
        exit 0
      fi

      checks=0

      ${builtins.concatStringsSep "\n\n" (
        map (serviceName: mkServiceProbeSection mode serviceName serviceSpecs.${serviceName}) serviceNames
      )}

      if [ "$checks" -eq 0 ]; then
        echo ${lib.escapeShellArg emptyMessage}
        exit 0
      fi

      printf '%s services=%s\n' ${lib.escapeShellArg successMessage} "$checks"
    '';

  portValueExpr =
    base: "$(( ${toString base} + env_offset + (slot_value * ${toString runtime.slot.stride}) ))";
  shellVar = name: "$" + name;

  mkTcpProbeBody =
    {
      serviceLabel,
      phaseLabel,
      successLabel,
      failureLabel,
      portVar,
      portBase,
    }:
    ''
      ${portVar}=${portValueExpr portBase}
      echo "INFO: checking ${serviceLabel} ${phaseLabel} port=${shellVar portVar} source=$service_source"
      if ${probeCommands.tcpOpenCmd {
        portExpr = shellVar portVar;
      }} then
        echo "OK: ${serviceLabel} ${successLabel} port=${shellVar portVar}"
      else
        echo "ERROR: ${serviceLabel} ${failureLabel} port=${shellVar portVar}"
        exit 1
      fi
    '';

  mkHttpProbeBody =
    {
      serviceLabel,
      phaseLabel,
      successLabel,
      failureLabel,
      path,
      portVar,
      portBase,
    }:
    ''
      ${portVar}=${portValueExpr portBase}
      echo "INFO: checking ${serviceLabel} ${phaseLabel} port=${shellVar portVar} source=$service_source"
      if ${probeCommands.httpGetOkCmd {
        urlExpr = "http://127.0.0.1:${shellVar portVar}${path}";
      }} then
        echo "OK: ${serviceLabel} ${successLabel} port=${shellVar portVar}"
      else
        echo "ERROR: ${serviceLabel} ${failureLabel} port=${shellVar portVar}"
        exit 1
      fi
    '';

  mkJsonRpcProbeBody =
    {
      serviceLabel,
      phaseLabel,
      successLabel,
      failureLabel,
      method,
      portVar,
      portBase,
    }:
    ''
      ${portVar}=${portValueExpr portBase}
      echo "INFO: checking ${serviceLabel} ${phaseLabel} port=${shellVar portVar} source=$service_source"
      if ${probeCommands.jsonRpcHasResultCmd {
        urlExpr = "http://127.0.0.1:${shellVar portVar}";
        inherit method;
      }} then
        echo "OK: ${serviceLabel} ${successLabel} port=${shellVar portVar}"
      else
        echo "ERROR: ${serviceLabel} ${failureLabel} port=${shellVar portVar}"
        exit 1
      fi
    '';

  postgresHealthBody = ''
    postgres_port=${portValueExpr postgresPortBase}
    echo "INFO: checking postgres health port=$postgres_port source=$service_source"
    if ${probeCommands.pgIsReadyCmd {
      postgres = postgresProbePkg;
      host = "127.0.0.1";
      portExpr = "$postgres_port";
    }} then
      echo "OK: postgres healthy port=$postgres_port"
    else
      echo "ERROR: postgres unhealthy port=$postgres_port"
      exit 1
    fi
  '';

  postgresReadyBody = ''
    postgres_port=${portValueExpr postgresPortBase}
    postgres_db=${lib.escapeShellArg postgresCfg.database}
    echo "INFO: checking postgres readiness port=$postgres_port source=$service_source"

    if ! ${probeCommands.pgIsReadyCmd {
      postgres = postgresProbePkg;
      host = "127.0.0.1";
      portExpr = "$postgres_port";
    }} then
      echo "ERROR: postgres not ready port=$postgres_port (pg_isready failed)"
      exit 1
    fi

    if ${probeCommands.psqlQueryCmd {
      postgres = postgresProbePkg;
      host = "127.0.0.1";
      portExpr = "$postgres_port";
      databaseExpr = "$postgres_db";
      query = "select 1;";
    }} >/dev/null 2>&1; then
      echo "OK: postgres ready port=$postgres_port database=$postgres_db"
    else
      echo "ERROR: postgres not ready port=$postgres_port database=$postgres_db (query failed)"
      exit 1
    fi
  '';

  healthProbeSpecs = {
    postgres = {
      count = 1;
      body = postgresHealthBody;
    };

    nginx = {
      count = 2;
      body = builtins.concatStringsSep "\n" [
        (mkTcpProbeBody {
          serviceLabel = "nginx";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "nginx_http_port";
          portBase = nginxHttpPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "nginx";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "nginx_https_port";
          portBase = nginxHttpsPortBase;
        })
      ];
    };

    minio = {
      count = 2;
      body = builtins.concatStringsSep "\n" [
        (mkTcpProbeBody {
          serviceLabel = "minio";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "minio_api_port";
          portBase = minioApiPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "minio";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "minio_console_port";
          portBase = minioConsolePortBase;
        })
      ];
    };

    reth = {
      count = 3;
      body = builtins.concatStringsSep "\n" [
        (mkJsonRpcProbeBody {
          serviceLabel = "reth";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          method = "web3_clientVersion";
          portVar = "reth_http_port";
          portBase = rethHttpPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "reth";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "reth_ws_port";
          portBase = rethWsPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "reth";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          portVar = "reth_auth_port";
          portBase = rethAuthPortBase;
        })
      ];
    };

    helios = {
      count = 2;
      body = builtins.concatStringsSep "\n" [
        (mkJsonRpcProbeBody {
          serviceLabel = "helios";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          method = "eth_chainId";
          portVar = "helios_rpc_port";
          portBase = heliosRpcPortBase;
        })
        (mkJsonRpcProbeBody {
          serviceLabel = "helios execution";
          phaseLabel = "health";
          successLabel = "healthy";
          failureLabel = "unhealthy";
          method = "web3_clientVersion";
          portVar = "helios_execution_rpc_port";
          portBase = heliosExecutionRpcPortBase;
        })
      ];
    };
  };

  readyProbeSpecs = {
    postgres = {
      count = 1;
      body = postgresReadyBody;
    };

    nginx = {
      count = 2;
      body = builtins.concatStringsSep "\n" [
        (mkTcpProbeBody {
          serviceLabel = "nginx";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "nginx_http_port";
          portBase = nginxHttpPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "nginx";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "nginx_https_port";
          portBase = nginxHttpsPortBase;
        })
      ];
    };

    minio = {
      count = 2;
      body = builtins.concatStringsSep "\n" [
        (mkTcpProbeBody {
          serviceLabel = "minio";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "minio_api_port";
          portBase = minioApiPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "minio";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "minio_console_port";
          portBase = minioConsolePortBase;
        })
      ];
    };

    reth = {
      count = 3;
      body = builtins.concatStringsSep "\n" [
        (mkJsonRpcProbeBody {
          serviceLabel = "reth";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          method = "eth_chainId";
          portVar = "reth_http_port";
          portBase = rethHttpPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "reth";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "reth_ws_port";
          portBase = rethWsPortBase;
        })
        (mkTcpProbeBody {
          serviceLabel = "reth";
          phaseLabel = "readiness";
          successLabel = "ready";
          failureLabel = "not ready";
          portVar = "reth_auth_port";
          portBase = rethAuthPortBase;
        })
      ];
    };

    helios = {
      count = 2;
      body = ''
        helios_source_kind_value="$(helios_source_kind "$service_source")"
        helios_readiness_profile=${lib.escapeShellArg heliosReadinessProfile}
        helios_require_not_syncing=${if heliosReadinessRequireNotSyncing then "1" else "0"}
        helios_rpc_port=$(( ${toString heliosRpcPortBase} + env_offset + (slot_value * ${toString runtime.slot.stride}) ))
        helios_execution_rpc_port=$(( ${toString heliosExecutionRpcPortBase} + env_offset + (slot_value * ${toString runtime.slot.stride}) ))
        echo "INFO: checking helios readiness port=$helios_rpc_port source=$service_source source_kind=$helios_source_kind_value profile=$helios_readiness_profile"
        if source_kind_disallowed "$helios_source_kind_value" ${heliosReadinessDisallowSourceKindArgs}; then
          echo "ERROR: helios not ready port=$helios_rpc_port source=$service_source source_kind=$helios_source_kind_value profile=$helios_readiness_profile (source kind disallowed)"
          exit 1
        fi

        helios_block_json="$(${probeCommands.jsonRpcRequestCmd {
          urlExpr = "http://127.0.0.1:$helios_rpc_port";
          method = "eth_blockNumber";
        }})" || true
        helios_block_number="$(printf '%s' "$helios_block_json" | ${pkgs.jq}/bin/jq -r '.result // empty')" || true
        if [ -z "$helios_block_number" ] || ! [[ "$helios_block_number" =~ ^0x[0-9a-fA-F]+$ ]]; then
          echo "ERROR: helios not ready port=$helios_rpc_port source=$service_source source_kind=$helios_source_kind_value (invalid eth_blockNumber result)"
          exit 1
        fi
        echo "OK: helios ready port=$helios_rpc_port block_number=$helios_block_number"

        if [ "$helios_require_not_syncing" = "1" ]; then
          helios_syncing_result="$(${probeCommands.jsonRpcFieldCmd {
            urlExpr = "http://127.0.0.1:$helios_rpc_port";
            method = "eth_syncing";
            jqExpr = ".result";
            raw = false;
          }})" || true
          if [ "$helios_syncing_result" != "false" ]; then
            echo "ERROR: helios not ready port=$helios_rpc_port source=$service_source source_kind=$helios_source_kind_value profile=$helios_readiness_profile (eth_syncing=$helios_syncing_result)"
            exit 1
          fi
          echo "OK: helios sync status ready port=$helios_rpc_port"
        else
          echo "SKIP: helios sync gate disabled profile=$helios_readiness_profile"
        fi

        echo "INFO: checking helios execution readiness port=$helios_execution_rpc_port source=$service_source"
        if ${probeCommands.jsonRpcHasResultCmd {
          urlExpr = "http://127.0.0.1:$helios_execution_rpc_port";
          method = "eth_chainId";
        }} then
          echo "OK: helios execution ready port=$helios_execution_rpc_port"
        else
          echo "ERROR: helios execution not ready port=$helios_execution_rpc_port"
          exit 1
        fi
      '';
    };
  };

  mkTask =
    {
      id,
      appName,
      summary,
      description,
      command,
      runtimeInputs ? [ ],
      contractArgs ? [ ],
    }:
    {
      inherit
        id
        summary
        description
        ;
      kind = "utility";
      runner = {
        type = "shell";
        command = command;
      };
      contract = {
        version = 1;
        input = {
          args = {
            parser = "typed";
            allowUnknown = false;
            spec = contractArgs;
          };
          env = {
            schemaRef = "runtimePrimitives";
            extra = [ ];
          };
        };
        output = {
          format = "text";
          channels = "stdout";
        };
        behavior = {
          idempotent = true;
          effects = [ "none" ];
          timeoutSec = 0;
        };
        errors.codes = {
          generic = 1;
          usage = 2;
          precondition = 3;
        };
      };
      runtime = {
        slotEnv = "optional";
        workdir = "projectRoot";
        hermetic = true;
        runtimeInputs = runtimeInputs;
        passThroughEnv = [
          runtime.env.var
          runtime.slot.var
        ];
        env = { };
        umask = "022";
        locale = "C.UTF-8";
        timezone = "UTC";
      };
      scheduling = {
        locks = [ ];
        maxAttempts = 1;
        retryBackoffSec = [ ];
        priority = 100;
      };
      deps = {
        needs = [ ];
        softNeeds = [ ];
      };
      produces = {
        artifacts = [ ];
        stateKeys = [ ];
      };
      ui.app = {
        expose = true;
        name = appName;
        category = "ops";
        usage = [ "nix run .#${appName}" ];
        examples = [ ];
      };
    };

  validateScript = ''
    set -euo pipefail

    slot_var=${lib.escapeShellArg runtime.slot.var}
    env_var=${lib.escapeShellArg runtime.env.var}

    slot_default=${toString runtime.slot.default}
    slot_max=${toString runtime.slot.max}
    env_default=${lib.escapeShellArg runtime.env.default}

    slot_value="''${!slot_var:-$slot_default}"
    env_value="''${!env_var:-$env_default}"

    if ! [[ "$slot_value" =~ ^[0-9]+$ ]]; then
      echo "ERROR: $slot_var must be an integer"
      exit 3
    fi

    if [ "$slot_value" -gt "$slot_max" ]; then
      echo "ERROR: $slot_var exceeds max slot ($slot_max)"
      exit 3
    fi

    case "$env_value" in
      ${envPattern}) ;;
      *)
        echo "ERROR: unsupported $env_var '$env_value'"
        exit 3
        ;;
    esac

    echo "OK: environment is valid (''${env_var}=$env_value ''${slot_var}=$slot_value)"
  '';

  portsScript = ''
        set -euo pipefail

        slot_var=${lib.escapeShellArg runtime.slot.var}
        env_var=${lib.escapeShellArg runtime.env.var}
        slot_default=${toString runtime.slot.default}
        env_default=${lib.escapeShellArg runtime.env.default}

        slot_value="''${!slot_var:-$slot_default}"
        env_value="''${!env_var:-$env_default}"

        case "$env_value" in
    ${envOffsetCase}
          *)
            echo "ERROR: unsupported $env_var '$env_value'"
            exit 3
            ;;
        esac

        echo "INFO: Port assignments for slot ''${slot_value} env ''${env_value}"
    ${portEmitLines}
  '';

  checkPortsScript = ''
        set -euo pipefail

    ${slotEnvPrelude}

        echo "INFO: Port status for slot ''${slot_value} env ''${env_value}"
    ${portCheckLines}
  '';

  healthScript = mkProbeScript {
    mode = "health";
    serviceSpecs = healthProbeSpecs;
    emptyMessage = "SKIP: no enabled services for health checks";
    successMessage = "OK: health checks passed";
  };

  readyScript = mkProbeScript {
    mode = "ready";
    serviceSpecs = readyProbeSpecs;
    emptyMessage = "SKIP: no enabled services for readiness checks";
    successMessage = "OK: readiness checks passed";
  };

  isolationScript = ''
    set -euo pipefail
    slot_var=${lib.escapeShellArg isolationSlotVar}
    env_var=${lib.escapeShellArg isolationEnvVar}
    slot_max=${toString isolationMaxSlot}
    max_parallel_default=${toString cfg.testIsolation.maxParallel}
    logs_root_base=${lib.escapeShellArg cfg.testIsolation.logsDir}
    run_task_id=${lib.escapeShellArg isolationRunTaskId}
    validate_task_id=${lib.escapeShellArg isolationValidateTaskId}
    keep_logs_success=${if cfg.testIsolation.keepLogsOnSuccess then "1" else "0"}
    keep_logs_failure=${if cfg.testIsolation.keepLogsOnFailure then "1" else "0"}

    slots_json='${isolationSlotsJson}'
    envs_json='${isolationEnvsJson}'
    run_args_json='${isolationRunArgsJson}'
    run_env_json='${isolationRunEnvJson}'
    selected_slot=""
    selected_env=""
    max_parallel_override=""
    effective_max_parallel=""
    executor_bin="''${NIXFIED_EXECUTOR_SELF:-''${NIXFIED_EXECUTOR_BIN:-}}"
    logs_root=""
    run_scope=""

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --slot)
          if [ "$#" -lt 2 ]; then
            echo "ERROR: --slot requires a value"
            exit 2
          fi
          selected_slot="$2"
          shift 2
          ;;
        --slot=*)
          selected_slot="''${1#--slot=}"
          shift
          ;;
        --env)
          if [ "$#" -lt 2 ]; then
            echo "ERROR: --env requires a value"
            exit 2
          fi
          selected_env="$2"
          shift 2
          ;;
        --env=*)
          selected_env="''${1#--env=}"
          shift
          ;;
        --max-parallel)
          if [ "$#" -lt 2 ]; then
            echo "ERROR: --max-parallel requires a value"
            exit 2
          fi
          max_parallel_override="$2"
          shift 2
          ;;
        --max-parallel=*)
          max_parallel_override="''${1#--max-parallel=}"
          shift
          ;;
        --)
          shift
          break
          ;;
        *)
          echo "ERROR: unknown argument '$1'"
          exit 2
          ;;
      esac
    done

    if [ "$#" -gt 0 ]; then
      echo "ERROR: unexpected positional arguments: $*"
      exit 2
    fi

    if [ -n "$selected_slot" ] && [ -z "$selected_env" ]; then
      echo "ERROR: --slot requires --env"
      exit 2
    fi

    if [ -n "$selected_env" ] && [ -z "$selected_slot" ]; then
      echo "ERROR: --env requires --slot"
      exit 2
    fi

    if [ -z "$run_task_id" ]; then
      echo "ERROR: test-isolation runTaskId is empty"
      exit 3
    fi

    if [ -z "$validate_task_id" ]; then
      echo "ERROR: test-isolation validateTaskId is empty"
      exit 3
    fi

    if [ -z "$executor_bin" ]; then
      echo "ERROR: test-isolation requires NIXFIED_EXECUTOR_SELF or NIXFIED_EXECUTOR_BIN"
      exit 3
    fi

    effective_max_parallel="$max_parallel_default"
    if [ -n "$max_parallel_override" ]; then
      effective_max_parallel="$max_parallel_override"
    fi
    if [ "''${CI:-}" = "1" ] || [ "''${CI:-}" = "true" ]; then
      effective_max_parallel=1
      echo "INFO: test-isolation forcing maxParallel=1 reason=ci"
    fi

    if ! [[ "$effective_max_parallel" =~ ^[0-9]+$ ]]; then
      echo "ERROR: test-isolation maxParallel is not an integer: $effective_max_parallel"
      exit 3
    fi

    if [ "$effective_max_parallel" -lt 1 ]; then
      echo "ERROR: test-isolation maxParallel must be >= 1"
      exit 3
    fi

    mapfile -t isolation_slots < <(${pkgs.jq}/bin/jq -r '.[]' <<<"$slots_json")
    mapfile -t isolation_envs < <(${pkgs.jq}/bin/jq -r '.[]' <<<"$envs_json")
    mapfile -t run_args < <(${pkgs.jq}/bin/jq -r '.[]' <<<"$run_args_json")
    mapfile -t run_env_entries < <(${pkgs.jq}/bin/jq -r 'to_entries[]? | [.key, (.value | tostring)] | @tsv' <<<"$run_env_json")

    if [ -n "$selected_slot" ]; then
      if ! [[ "$selected_slot" =~ ^[0-9]+$ ]]; then
        echo "ERROR: --slot must be an integer"
        exit 2
      fi
      filtered_slots=()
      for slot_value in "''${isolation_slots[@]}"; do
        if [ "$slot_value" = "$selected_slot" ]; then
          filtered_slots+=("$slot_value")
        fi
      done
      isolation_slots=("''${filtered_slots[@]}")
      if [ "''${#isolation_slots[@]}" -eq 0 ]; then
        echo "ERROR: selected slot is not in the isolation matrix: $selected_slot"
        exit 3
      fi
    fi

    if [ -n "$selected_env" ]; then
      filtered_envs=()
      for env_value in "''${isolation_envs[@]}"; do
        if [ "$env_value" = "$selected_env" ]; then
          filtered_envs+=("$env_value")
        fi
      done
      isolation_envs=("''${filtered_envs[@]}")
      if [ "''${#isolation_envs[@]}" -eq 0 ]; then
        echo "ERROR: selected environment is not in the isolation matrix: $selected_env"
        exit 3
      fi
    fi

    if [ "''${#isolation_slots[@]}" -eq 0 ]; then
      echo "ERROR: test-isolation matrix has no slots"
      exit 3
    fi

    if [ "''${#isolation_envs[@]}" -eq 0 ]; then
      echo "ERROR: test-isolation matrix has no environments"
      exit 3
    fi

    run_scope="''${NIXFIED_RUN_ID:-run-$(${pkgs.coreutils}/bin/date -u +%Y%m%d-%H%M%S)-$$}"
    logs_root="$logs_root_base/$run_scope"
    mkdir -p "$logs_root"
    echo "INFO: test-isolation matrix slots=''${#isolation_slots[@]} envs=''${#isolation_envs[@]} max_parallel=$effective_max_parallel"
    echo "INFO: test-isolation logs_root=$logs_root"

    statuses_dir="$(mktemp -d "$logs_root/.status.XXXXXX")"
    semaphore_dir="$(mktemp -d "$logs_root/.semaphore.XXXXXX")"
    semaphore_fifo="$semaphore_dir/tokens.fifo"
    mkfifo "$semaphore_fifo"
    exec 9<>"$semaphore_fifo"
    rm -f "$semaphore_fifo"

    token_count=0
    while [ "$token_count" -lt "$effective_max_parallel" ]; do
      printf 'token\n' >&9
      token_count=$((token_count + 1))
    done

    total=0
    failed=0
    worker_pids=()
    status_files=()

    for slot_value in "''${isolation_slots[@]}"; do
      if ! [[ "$slot_value" =~ ^[0-9]+$ ]]; then
        echo "ERROR: matrix slot is not an integer: $slot_value"
        failed=$((failed + 1))
        continue
      fi
      if [ "$slot_value" -gt "$slot_max" ]; then
        echo "ERROR: matrix slot exceeds max slot ($slot_max): $slot_value"
        failed=$((failed + 1))
        continue
      fi

      for env_value in "''${isolation_envs[@]}"; do
        total=$((total + 1))

        case "$env_value" in
          ${envPattern})
            ;;
          *)
            echo "ERROR: unsupported environment in matrix: $env_value"
            failed=$((failed + 1))
            continue
            ;;
        esac

        cell_name="slot-''${slot_value}__env-''${env_value}"
        cell_dir="$logs_root/$cell_name"
        registry_dir="$cell_dir/registry"
        artifacts_dir="$cell_dir/artifacts"
        validate_log="$cell_dir/validate.log"
        run_log="$cell_dir/run.log"
        validate_run_id_file="$cell_dir/validate.run-id"
        run_id_file="$cell_dir/run.run-id"
        summary_file="$cell_dir/summary.json"
        status_file="$statuses_dir/$cell_name.rc"

        mkdir -p "$cell_dir" "$registry_dir" "$artifacts_dir"
        echo "INFO: isolation cell start slot=$slot_value env=$env_value"
        status_files+=("$status_file")

        IFS= read -r -u 9 _
        (
          set +e
          rc=1
          runtime_root="$cell_dir/runtime"
          services_root="$cell_dir/services"
          mkdir -p \
            "$runtime_root/home" \
            "$runtime_root/tmp" \
            "$runtime_root/xdg/data" \
            "$runtime_root/xdg/state" \
            "$runtime_root/xdg/cache" \
            "$services_root"
          export "$slot_var=$slot_value"
          export "$env_var=$env_value"
          export HOME="$runtime_root/home"
          export TMPDIR="$runtime_root/tmp"
          export XDG_DATA_HOME="$runtime_root/xdg/data"
          export XDG_STATE_HOME="$runtime_root/xdg/state"
          export XDG_CACHE_HOME="$runtime_root/xdg/cache"
          export REGISTRY_ROOT="$registry_dir"
          export CI_ARTIFACTS_DIR="$artifacts_dir"
          export NIXFIED_SERVICE_ROOT="$services_root"
          export NIXFIED_RUNTIME_HOME="$HOME"
          export NIXFIED_RUNTIME_TMPDIR="$TMPDIR"
          export NIXFIED_RUNTIME_XDG_DATA_HOME="$XDG_DATA_HOME"
          export NIXFIED_RUNTIME_XDG_STATE_HOME="$XDG_STATE_HOME"
          export NIXFIED_RUNTIME_XDG_CACHE_HOME="$XDG_CACHE_HOME"
          export NIXFIED_RUNTIME_REGISTRY_ROOT="$registry_dir"
          export NIXFIED_RUNTIME_ARTIFACTS_DIR="$artifacts_dir"
          export NIXFIED_RUNTIME_SERVICE_ROOT="$services_root"

          for run_env_entry in "''${run_env_entries[@]}"; do
            run_env_key="''${run_env_entry%%$'\t'*}"
            run_env_value="''${run_env_entry#*$'\t'}"
            export "$run_env_key=$run_env_value"
          done

          NIXFIED_CALLER_PWD="$PWD" "$executor_bin" run-task "$validate_task_id" --run-id-file "$validate_run_id_file" > "$validate_log" 2>&1
          rc="$?"
          if [ "$rc" -eq 0 ]; then
            NIXFIED_CALLER_PWD="$PWD" "$executor_bin" run-task "$run_task_id" "''${run_args[@]}" --run-id-file "$run_id_file" --summary-file "$summary_file" > "$run_log" 2>&1
            rc="$?"
          fi

          printf '%s\n' "$rc" > "$status_file"
          if [ "$rc" -eq 0 ]; then
            echo "OK: isolation cell passed slot=$slot_value env=$env_value"
            if [ "$keep_logs_success" -eq 0 ]; then
              rm -rf "$cell_dir"
            fi
          else
            echo "ERROR: isolation cell failed slot=$slot_value env=$env_value rc=$rc"
          fi

          printf 'token\n' >&9
          exit 0
        ) &
        worker_pids+=("$!")
      done
    done

    for worker_pid in "''${worker_pids[@]}"; do
      wait "$worker_pid" || true
    done

    exec 9>&-
    exec 9<&-
    rm -rf "$semaphore_dir"

    for status_file in "''${status_files[@]}"; do
      if [ ! -f "$status_file" ]; then
        failed=$((failed + 1))
        echo "ERROR: isolation cell status missing file=$status_file"
        continue
      fi

      rc="$(cat "$status_file")"
      if [ "$rc" != "0" ]; then
        failed=$((failed + 1))
      fi
    done
    rm -rf "$statuses_dir"

    if [ "$total" -eq 0 ]; then
      echo "ERROR: test-isolation matrix did not execute any cells"
      exit 3
    fi

    if [ "$failed" -ne 0 ]; then
      echo "ERROR: test-isolation completed with failures failed=$failed total=$total"
      if [ "$keep_logs_failure" -eq 0 ]; then
        rm -rf "$logs_root"
      fi
      exit 1
    fi

    if [ "$keep_logs_success" -eq 1 ]; then
      echo "INFO: isolation logs preserved at $logs_root"
    else
      rm -rf "$logs_root"
    fi
    echo "OK: test-isolation completed total=$total"
  '';
in
{
  options.nixfied.operations = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    validateEnv.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    testIsolation.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    testIsolation.slots = lib.mkOption {
      type = lib.types.listOf lib.types.int;
      default = [ runtime.slot.default ];
    };

    testIsolation.envs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = runtime.env.names;
    };

    testIsolation.logsDir = lib.mkOption {
      type = lib.types.str;
      default = "/tmp/${config.nixfied.identity.projectId}-isolation";
    };

    testIsolation.keepLogsOnSuccess = lib.mkOption {
      type = lib.types.bool;
      default = false;
    };

    testIsolation.keepLogsOnFailure = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    testIsolation.maxParallel = lib.mkOption {
      type = lib.types.ints.positive;
      default = 4;
    };

    testIsolation.runTaskId = lib.mkOption {
      type = lib.types.str;
      default = "task.ci";
    };

    testIsolation.runApp = lib.mkOption {
      type = lib.types.str;
      default = "ci";
    };

    testIsolation.runArgs = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "--summary" ];
    };

    testIsolation.validateApp = lib.mkOption {
      type = lib.types.str;
      default = "validate-env";
    };

    testIsolation.validateTaskId = lib.mkOption {
      type = lib.types.str;
      default = "task.ops.validate-env";
    };

    testIsolation.runEnv = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.oneOf [
          lib.types.str
          lib.types.int
          lib.types.bool
        ]
      );
      default = { };
    };

    ports.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    checkPorts.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    health.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };

    ready.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
    };
  };

  config = lib.mkMerge [
    (lib.mkIf (cfg.enable && cfg.validateEnv.enable) {
      nixfied.tasks."validate-env" = mkTask {
        id = "task.ops.validate-env";
        appName = "validate-env";
        summary = "Validate slot/env settings";
        description = "Validates PROJECT_ENV and NIX_ENV values against model runtime constraints.";
        command = validateScript;
      };
    })

    (lib.mkIf (cfg.enable && cfg.testIsolation.enable) {
      nixfied.tasks."test-isolation" = mkTask {
        id = "task.ops.test-isolation";
        appName = "test-isolation";
        summary = "Run isolation checks";
        description = "Runs deterministic isolation smoke checks from model metadata.";
        command = isolationScript;
        contractArgs = testIsolationContractArgs;
        runtimeInputs = [
          pkgs.coreutils
          pkgs.jq
        ];
      };
    })

    (lib.mkIf (cfg.enable && cfg.ports.enable) {
      nixfied.tasks."ports" = mkTask {
        id = "task.ops.ports";
        appName = "ports";
        summary = "Print model-derived port assignments";
        description = "Prints computed per-slot/per-env ports from compiled runtime data.";
        command = portsScript;
      };
    })

    (lib.mkIf (cfg.enable && cfg.checkPorts.enable) {
      nixfied.tasks."check-ports" = mkTask {
        id = "task.ops.check-ports";
        appName = "check-ports";
        summary = "Check model-derived port availability";
        description = "Checks if computed per-slot/per-env ports are listening or free.";
        command = checkPortsScript;
        runtimeInputs = [
          pkgs.coreutils
          pkgs.gnugrep
          pkgs.gnused
          (if pkgs ? lsof then pkgs.lsof else pkgs.coreutils)
        ];
      };
    })

    (lib.mkIf (cfg.enable && cfg.health.enable) {
      nixfied.tasks."health" = mkTask {
        id = "task.ops.health";
        appName = "health";
        summary = "Run service health checks";
        description = ''
          Runs health checks for selected enabled services:
          postgres, nginx (http+https), minio (api+console),
          reth (http+ws+auth), and helios (rpc+execution).
          Optional selectors: --service <name|all> and --source <key>.
        '';
        command = healthScript;
        runtimeInputs = serviceProbeRuntimeInputs;
        contractArgs = serviceSelectionContractArgs;
      };
    })

    (lib.mkIf (cfg.enable && cfg.ready.enable) {
      nixfied.tasks."ready" = mkTask {
        id = "task.ops.ready";
        appName = "ready";
        summary = "Run service readiness checks";
        description = ''
          Runs readiness checks for selected enabled services:
          postgres, nginx (http+https), minio (api+console),
          reth (http+ws+auth), and helios (rpc+execution).
          Optional selectors: --service <name|all> and --source <key>.
        '';
        command = readyScript;
        runtimeInputs = serviceProbeRuntimeInputs;
        contractArgs = serviceSelectionContractArgs;
      };
    })
  ];
}
