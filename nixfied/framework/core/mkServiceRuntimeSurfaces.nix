{
  pkgs,
  model,
  services,
  selectedServices ? null,
  serviceSurfaceCatalog ? null,
}:
let
  lib = pkgs.lib;
  listUtils = import ./list-utils.nix;
  commonRuntimeShell = import ../runtime/common-runtime.nix { inherit pkgs; };
  shellJson = import ../runtime/helpers/shell-json.nix { };

  tokenLib = import ../runtime/helpers/normalize-token.nix { inherit lib; };
  normalizeToken = tokenLib.normalizeToken;

  serviceIds = builtins.sort builtins.lessThan (builtins.attrNames (services));
  selectedServiceNames =
    if selectedServices == null then null else listUtils.uniqueSorted selectedServices;
  selectedServiceSet =
    if selectedServiceNames == null then
      { }
    else
      builtins.listToAttrs (
        map (serviceName: {
          name = serviceName;
          value = true;
        }) selectedServiceNames
      );
  knownServiceNames = lib.unique (
    map (
      serviceId:
      let
        service = services.${serviceId};
      in
      service.name or serviceId
    ) serviceIds
  );
  unknownSelectedServices =
    if selectedServiceNames == null then
      [ ]
    else
      builtins.filter (
        serviceName:
        !(builtins.elem serviceName knownServiceNames) && !(builtins.elem serviceName serviceIds)
      ) selectedServiceNames;
  enabledServiceIds = builtins.filter (
    serviceId:
    let
      service = services.${serviceId};
      serviceName = service.name or serviceId;
    in
    (service.enable or false)
    && (
      selectedServiceNames == null
      || builtins.hasAttr serviceName selectedServiceSet
      || builtins.hasAttr serviceId selectedServiceSet
    )
  ) serviceIds;

  validateRuntimeSurfaceService =
    entry:
    let
      service = services.${entry.id};
      config = service.config or { };
      fail =
        requirement:
        throw ''
          nixfied service runtime surfaces: enabled service '${entry.name}' is missing ${requirement}.
          Configure nixfied.services.${entry.name}.sources.<source>.package, packageAttr, or packageFactory and defaultSource, or disable the service.
        '';
    in
    if entry.name == "nginx" then
      if (config.package or null) != null || pkgs ? nginx then true else fail "a runtime package"
    else if entry.name == "reth" then
      if (config.package or null) != null || pkgs ? reth then true else fail "a runtime package"
    else if entry.name == "minio" then
      if
        ((config.package or null) != null || pkgs ? minio)
        && ((config.clientPackage or null) != null || pkgs ? minio-client)
      then
        true
      else if !((config.package or null) != null || pkgs ? minio) then
        fail "a server package"
      else
        fail "a client package"
    else
      true;

  serviceEntries =
    let
      entries = map (
        serviceId:
        let
          service = services.${serviceId};
          serviceName = service.name or serviceId;
          dataDirName = service.config.dataDirName or serviceName;
        in
        {
          id = serviceId;
          name = serviceName;
          token = normalizeToken serviceName;
          inherit dataDirName;
        }
      ) enabledServiceIds;
      duplicateDataDirNames = lib.unique (
        builtins.filter (
          dataDirName: builtins.length (builtins.filter (entry: entry.dataDirName == dataDirName) entries) > 1
        ) (map (entry: entry.dataDirName) entries)
      );
    in
    if unknownSelectedServices != [ ] then
      throw "nixfied service runtime surfaces received unknown selected services: ${builtins.concatStringsSep ", " unknownSelectedServices}"
    else if duplicateDataDirNames == [ ] then
      map (entry: builtins.seq (validateRuntimeSurfaceService entry) entry) entries
    else
      throw "nixfied service runtime surfaces require unique dataDirName values, duplicates: ${builtins.concatStringsSep ", " duplicateDataDirNames}";

  portNames = builtins.sort builtins.lessThan (builtins.attrNames (model.runtime.ports or { }));
  envNames = builtins.sort builtins.lessThan (builtins.attrNames (model.runtime.env.offsets or { }));

  envOffsetsTsv = builtins.concatStringsSep "\n" (
    map (envName: "${envName}\t${toString model.runtime.env.offsets.${envName}}") envNames
  );
  portBasesTsv = builtins.concatStringsSep "\n" (
    map (portName: "${portName}\t${toString model.runtime.ports.${portName}}") portNames
  );
  serviceDataDirMapTsv = builtins.concatStringsSep "\n" (
    map (entry: "${entry.dataDirName}\t${entry.token}") serviceEntries
  );

  slotRuntimePrelude = ''
    SLOT_RUNTIME_ENV_OFFSETS_TSV=${lib.escapeShellArg envOffsetsTsv}
    SLOT_RUNTIME_PORT_BASES_TSV=${lib.escapeShellArg portBasesTsv}
    SLOT_RUNTIME_SERVICE_DATA_DIR_MAP_TSV=${lib.escapeShellArg serviceDataDirMapTsv}
    SLOT_RUNTIME_SLOT_VAR=${lib.escapeShellArg model.runtime.slot.var}
    SLOT_RUNTIME_ENV_VAR=${lib.escapeShellArg model.runtime.env.var}
    SLOT_RUNTIME_SLOT_DEFAULT=${lib.escapeShellArg (toString model.runtime.slot.default)}
    SLOT_RUNTIME_ENV_DEFAULT=${lib.escapeShellArg model.runtime.env.default}
    SLOT_RUNTIME_SLOT_STRIDE=${lib.escapeShellArg (toString model.runtime.slot.stride)}
    SLOT_RUNTIME_DIR_BASE=${lib.escapeShellArg model.runtime.directories.base}

    normalize_slot_runtime_token() {
      printf '%s' "$1" | ${pkgs.coreutils}/bin/tr '[:lower:].-:/ ' '[:upper:]______' | ${pkgs.coreutils}/bin/tr -c 'A-Z0-9_' '_'
    }

    resolve_slot_runtime_context() {
      local slot_var="$SLOT_RUNTIME_SLOT_VAR"
      local env_var="$SLOT_RUNTIME_ENV_VAR"
      local slot_default="$SLOT_RUNTIME_SLOT_DEFAULT"
      local env_default="$SLOT_RUNTIME_ENV_DEFAULT"
      local slot_stride="$SLOT_RUNTIME_SLOT_STRIDE"
      local runtime_dir_base="$SLOT_RUNTIME_DIR_BASE"
      local env_offset=""
      local line=""

      SLOT_RUNTIME_SLOT_VALUE="''${!slot_var:-$slot_default}"
      SLOT_RUNTIME_ENV_VALUE="''${!env_var:-$env_default}"

      if ! [[ "$SLOT_RUNTIME_SLOT_VALUE" =~ ^[0-9]+$ ]]; then
        echo "ERROR: $slot_var must be an integer" >&2
        exit 3
      fi

      while IFS=$'\t' read -r env_name env_offset_value || [ -n "$env_name" ]; do
        if [ -z "$env_name" ]; then
          continue
        fi
        if [ "$env_name" = "$SLOT_RUNTIME_ENV_VALUE" ]; then
          env_offset="$env_offset_value"
          break
        fi
      done <<< "$SLOT_RUNTIME_ENV_OFFSETS_TSV"

      if [ -z "$env_offset" ]; then
        echo "ERROR: unsupported $env_var '$SLOT_RUNTIME_ENV_VALUE'" >&2
        exit 3
      fi

      if [ -n "''${NIXFIED_RUNTIME_DIR_BASE+x}" ]; then
        runtime_dir_base="$NIXFIED_RUNTIME_DIR_BASE"
      fi

      SLOT_RUNTIME_SCOPE_ROOT="''${NIXFIED_RUNTIME_DIR_SCOPE:-''${NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE:-}}"
      if [ -z "$SLOT_RUNTIME_SCOPE_ROOT" ]; then
        SLOT_RUNTIME_SCOPE_ROOT="$runtime_dir_base/$SLOT_RUNTIME_ENV_VALUE/slot-$SLOT_RUNTIME_SLOT_VALUE"
      fi

      SLOT_RUNTIME_RUN_DIR="$SLOT_RUNTIME_SCOPE_ROOT/run"
      SLOT_RUNTIME_LOG_DIR="$SLOT_RUNTIME_SCOPE_ROOT/log"
      SLOT_RUNTIME_CONFIG_DIR="$SLOT_RUNTIME_SCOPE_ROOT/config"
      SLOT_RUNTIME_SERVICE_ROOT="$SLOT_RUNTIME_SCOPE_ROOT/services"

      mkdir -p "$SLOT_RUNTIME_RUN_DIR" "$SLOT_RUNTIME_LOG_DIR" "$SLOT_RUNTIME_CONFIG_DIR" "$SLOT_RUNTIME_SERVICE_ROOT"
    }

    resolve_slot_runtime_port_value() {
      local port_key="$1"
      local env_offset=""
      local line=""

      while IFS=$'\t' read -r port_name port_base || [ -n "$port_name" ]; do
        if [ -z "$port_name" ]; then
          continue
        fi
        if [ "$port_name" = "$port_key" ]; then
          while IFS=$'\t' read -r env_name env_offset_value || [ -n "$env_name" ]; do
            if [ -z "$env_name" ]; then
              continue
            fi
            if [ "$env_name" = "$SLOT_RUNTIME_ENV_VALUE" ]; then
              env_offset="$env_offset_value"
              break
            fi
          done <<< "$SLOT_RUNTIME_ENV_OFFSETS_TSV"

          if [ -z "$env_offset" ]; then
            echo "ERROR: unsupported $SLOT_RUNTIME_ENV_VAR '$SLOT_RUNTIME_ENV_VALUE'" >&2
            exit 3
          fi

          printf '%s' "$(( port_base + env_offset + (SLOT_RUNTIME_SLOT_VALUE * SLOT_RUNTIME_SLOT_STRIDE) ))"
          return 0
        fi
      done <<< "$SLOT_RUNTIME_PORT_BASES_TSV"

      echo "ERROR: unsupported runtime port key '$port_key'" >&2
      exit 3
    }

    resolve_slot_runtime_service_token() {
      local data_dir_name="$1"

      while IFS=$'\t' read -r mapped_data_dir mapped_token || [ -n "$mapped_data_dir" ]; do
        if [ -z "$mapped_data_dir" ]; then
          continue
        fi
        if [ "$mapped_data_dir" = "$data_dir_name" ]; then
          printf '%s' "$mapped_token"
          return 0
        fi
      done <<< "$SLOT_RUNTIME_SERVICE_DATA_DIR_MAP_TSV"

      normalize_slot_runtime_token "$data_dir_name"
    }
  '';

  slotInfoJson = pkgs.writeShellScript "nixfied-slot-info-json" ''
    set -euo pipefail
    ${commonRuntimeShell}
    ${slotRuntimePrelude}
    ${shellJson}

    resolve_slot_runtime_context

    printf '{'
    printf '"slot":%s' "$(json_quote_string "$SLOT_RUNTIME_SLOT_VALUE")"
    printf ',"env":%s' "$(json_quote_string "$SLOT_RUNTIME_ENV_VALUE")"
    printf ',"ports":{'
    first_port=1
    while IFS=$'\t' read -r port_name port_base || [ -n "$port_name" ]; do
      port_var=""
      port_value=""

      if [ -z "$port_name" ]; then
        continue
      fi

      port_var="$(normalize_slot_runtime_token "$port_name")_PORT"
      port_value="$(resolve_slot_runtime_port_value "$port_name")"
      if [ "$first_port" -eq 0 ]; then
        printf ','
      fi
      printf '%s:%s' "$(json_quote_string "$port_var")" "$port_value"
      first_port=0
    done <<< "$SLOT_RUNTIME_PORT_BASES_TSV"
    printf '}'
    printf ',"directories":{'
    printf '"run":%s' "$(json_quote_string "$SLOT_RUNTIME_RUN_DIR")"
    printf ',"log":%s' "$(json_quote_string "$SLOT_RUNTIME_LOG_DIR")"
    printf ',"config":%s' "$(json_quote_string "$SLOT_RUNTIME_CONFIG_DIR")"
    printf '}'
    printf ',"vars":{'
    printf '"NIXFIED_RUNTIME_DIR_SCOPE":%s' "$(json_quote_string "$SLOT_RUNTIME_SCOPE_ROOT")"
    printf ',"NIXFIED_SERVICE_ROOT":%s' "$(json_quote_string "$SLOT_RUNTIME_SERVICE_ROOT")"
    printf '}'
    printf '}\n'
  '';

  slotInfo = pkgs.writeShellScript "nixfied-slot-info" ''
    set -euo pipefail
    ${slotRuntimePrelude}

    resolve_slot_runtime_context

    printf 'SLOT=%q\n' "$SLOT_RUNTIME_SLOT_VALUE"
    printf 'ENV=%q\n' "$SLOT_RUNTIME_ENV_VALUE"
    printf 'RUN_DIR=%q\n' "$SLOT_RUNTIME_RUN_DIR"
    printf 'LOG_DIR=%q\n' "$SLOT_RUNTIME_LOG_DIR"
    printf 'CONFIG_DIR=%q\n' "$SLOT_RUNTIME_CONFIG_DIR"
    printf 'NIXFIED_RUNTIME_DIR_SCOPE=%q\n' "$SLOT_RUNTIME_SCOPE_ROOT"
    printf 'NIXFIED_SERVICE_ROOT=%q\n' "$SLOT_RUNTIME_SERVICE_ROOT"
    while IFS=$'\t' read -r port_name port_base || [ -n "$port_name" ]; do
      port_var=""
      port_value=""

      if [ -z "$port_name" ]; then
        continue
      fi

      port_var="$(normalize_slot_runtime_token "$port_name")_PORT"
      port_value="$(resolve_slot_runtime_port_value "$port_name")"
      printf '%s=%q\n' "$port_var" "$port_value"
    done <<< "$SLOT_RUNTIME_PORT_BASES_TSV"
  '';

  serviceDirResolver = pkgs.writeShellScript "nixfied-service-dir" ''
    set -euo pipefail
    ${slotRuntimePrelude}

    data_dir_name="''${1:-}"
    if [ -z "$data_dir_name" ]; then
      echo "ERROR: missing service data dir name" >&2
      exit 2
    fi

    resolve_slot_runtime_context

    service_token="$(resolve_slot_runtime_service_token "$data_dir_name")"
    service_env_var="NIXFIED_SERVICE_''${service_token}_DATA_DIR"

    if [ -n "''${!service_env_var:-}" ]; then
      printf '%s' "''${!service_env_var}"
      exit 0
    fi

    printf '%s/%s/data' "$SLOT_RUNTIME_SERVICE_ROOT" "$data_dir_name"
  '';

  serviceProject = {
    project = {
      id = model.identity.projectId;
      slotVar = model.runtime.slot.var;
      envVar = model.runtime.env.var;
    };
    logging = {
      level = model.runtime.logging.levelDefault;
      output = model.runtime.logging.outputDefault;
    };
    state = {
      policy = model.state.policy;
    };
    ci = {
      artifacts = {
        dir = model.state.policy.artifactsRoot;
      };
    };
    directories = {
      base = model.runtime.directories.base;
    };
    services =
      if selectedServiceNames == null then
        services
      else
        lib.filterAttrs (
          serviceId: service:
          let
            serviceName = service.name or serviceId;
          in
          builtins.hasAttr serviceName selectedServiceSet || builtins.hasAttr serviceId selectedServiceSet
        ) services;
  };

  slots = {
    getSlotInfo = slotInfo;
    getSlotInfoJson = slotInfoJson;
    portVarName = portKey: "${normalizeToken portKey}_PORT";
    getServiceDir = dataDirName: "$(${serviceDirResolver} ${lib.escapeShellArg dataDirName})";
  };

  shellContract = import ../runtime/helpers/shell-contract.nix { inherit pkgs; };
  loggingRuntime = import ../runtime/helpers/logging-runtime.nix { inherit pkgs; };
  summary = import ../runtime/helpers/summary.nix {
    inherit pkgs;
    project = serviceProject;
    inherit (loggingRuntime) loggingPrelude;
  };
  helpers = import ../runtime/helpers/helpers.nix {
    inherit pkgs;
    project = serviceProject;
    hooks = { };
    inherit (summary) summaryParser;
  };
  fixtures = import ../runtime/helpers/fixtures.nix {
    inherit pkgs;
    project = serviceProject;
  };
  builders = import ../runtime/helpers/builders.nix {
    inherit
      pkgs
      shellContract
      ;
    project = serviceProject;
    fixtureLib = fixtures;
    inherit (helpers)
      loadEnv
      loadEnvFile
      helpersScript
      hookExports
      ;
  };
  appApi = import ../runtime/helpers/app-api.nix {
    inherit
      pkgs
      shellContract
      ;
    inherit (builders) mkApp;
  };
  serviceApi = import ../runtime/helpers/service-api.nix {
    inherit
      pkgs
      shellContract
      ;
  };
  runtimeEvents = import ../runtime/helpers/runtime-events.nix {
    inherit
      pkgs
      ;
    project = serviceProject;
  };
  observability = import ../runtime/helpers/service-observability.nix {
    inherit
      pkgs
      slots
      runtimeEvents
      ;
  };

  serviceApiCatalogEntries =
    let
      requestedServiceNames = builtins.map (entry: entry.name) serviceEntries;
      compiledServiceApis =
        if serviceSurfaceCatalog == null then null else serviceSurfaceCatalog.serviceApis or { };
      compiledOperationCatalog =
        if serviceSurfaceCatalog == null then null else serviceSurfaceCatalog.operationCatalog or { };
      missingServiceApis = builtins.filter (
        name: !(builtins.hasAttr name compiledServiceApis)
      ) requestedServiceNames;
      missingOperationCatalogs = builtins.filter (
        name: !(builtins.hasAttr name compiledOperationCatalog)
      ) requestedServiceNames;
      serviceEntriesFromCatalog = builtins.listToAttrs (
        map (serviceName: {
          name = serviceName;
          value = compiledServiceApis.${serviceName};
        }) requestedServiceNames
      );
    in
    if serviceSurfaceCatalog == null then
      throw "nixfied service runtime surfaces require compiled serviceSurfaceCatalog"
    else if requestedServiceNames == [ ] then
      { }
    else if missingOperationCatalogs != [ ] then
      throw "nixfied service runtime surfaces expected operation catalogs for all selected services in serviceSurfaceCatalog: ${builtins.concatStringsSep ", " missingOperationCatalogs}"
    else if missingServiceApis == [ ] then
      serviceEntriesFromCatalog
    else
      throw "nixfied service runtime surfaces expected service APIs for all selected services in serviceSurfaceCatalog: ${builtins.concatStringsSep ", " missingServiceApis}";

  mergeAdapterOperations =
    serviceName: baseOps: extraOps:
    let
      duplicateOps = builtins.filter (opName: builtins.hasAttr opName baseOps) (
        builtins.attrNames extraOps
      );
    in
    if duplicateOps == [ ] then
      baseOps // extraOps
    else
      throw "nixfied service runtime adapter for '${serviceName}' defines duplicate operations: ${builtins.concatStringsSep ", " duplicateOps}";

  serviceAdapters = builtins.mapAttrs (
    serviceName: contract:
    let
      adapterModule = (contract.adapter or { }).module or null;
      importedAdapter =
        if adapterModule == null then
          throw "nixfied service runtime surfaces require adapter.module for service '${serviceName}'"
        else
          import adapterModule {
            inherit
              pkgs
              slots
              ;
            project = serviceProject;
          };
      observabilityOps = {
        log = observability.mkLogScript serviceName;
        events = observability.mkEventsScript serviceName;
      };
    in
    {
      version = importedAdapter.version or 1;
      operations = mergeAdapterOperations serviceName (importedAdapter.operations or { }
      ) observabilityOps;
    }
  ) serviceApiCatalogEntries;

  serviceApis = serviceApiCatalogEntries;
  serviceOperationCatalogEntries = builtins.listToAttrs (
    map (serviceName: {
      name = serviceName;
      value = serviceSurfaceCatalog.operationCatalog.${serviceName};
    }) (builtins.attrNames serviceApis)
  );
  serviceOps = serviceApi.collectServiceOpsFromCatalog {
    serviceContracts = serviceApis;
    operationCatalog = serviceOperationCatalogEntries;
    inherit serviceAdapters;
  };
  serviceHookEnv = serviceApi.mkServiceHookEnv serviceOps;
  mkServiceRuntimeApp =
    op:
    (
      appApi.mkContractBackedApp {
        name = op.appName;
        script = ''
          exec ${toString op.launcher} "$@"
        '';
        contract = {
          class = op.class;
          summary = op.opMetadata.summary;
          details = op.opMetadata.details;
          usage = op.usage;
          examples = op.opMetadata.examples or [ ];
          args = op.opMetadata.args or [ ];
          env = op.opMetadata.env or [ ];
          category = op.category;
          idempotent = op.idempotent;
        };
        env = { };
        useDeps = false;
        meta = {
          nixfied = {
            service = op.serviceName;
            operation = op.opName;
          };
        };
      }
    ).program;
  serviceAppPrograms = builtins.listToAttrs (
    map (op: {
      name = op.appName;
      value = mkServiceRuntimeApp op;
    }) (builtins.filter (op: op.includeApp) serviceOps)
  );
in
{
  inherit
    serviceApis
    serviceAppPrograms
    serviceHookEnv
    slots
    ;
}
