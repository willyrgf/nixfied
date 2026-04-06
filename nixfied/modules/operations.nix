{
  lib,
  config,
  pkgs,
  ...
}:
let
  cfg = config.nixfied.operations;
  inherit (config.nixfied) runtime;
  inherit (config.nixfied) services;
  exitCodes = import ../framework/core/exit-codes.nix;
  shellCommon = import ../framework/core/shell-common.nix { inherit pkgs; };
  skipPolicy = import ../framework/core/skip-policy.nix { inherit pkgs; };
  testIsolationRuntime = import ./operations/test-isolation-runtime.nix {
    inherit lib pkgs;
  };
  serviceConfigLib = import ../framework/core/service-config.nix { inherit lib pkgs; };

  configuredServiceNames = builtins.sort builtins.lessThan (builtins.attrNames services);
  excludedServices = config.nixfied.graph.excludedServices or [ ];
  serviceNames = builtins.filter (
    serviceName: !(builtins.elem serviceName excludedServices)
  ) configuredServiceNames;

  resolvePortBase =
    key:
    if builtins.hasAttr key runtime.ports then
      runtime.ports.${key}
    else
      throw "nixfied.operations: port key '${key}' is not defined in nixfied.runtime.ports";

  netcatPkg =
    pkgs.netcat or (pkgs.netcat-openbsd or (throw "nixfied.operations: netcat package is required for readiness probes"));

  serviceCheckRuntimeInputs = builtins.concatLists (
    map (
      serviceName:
      (serviceConfigLib.normalizeServiceConfig {
        name = serviceName;
        config = services.${serviceName};
      }).checkRuntimeInputs or [ ]
    ) serviceNames
  );
  serviceProbeRuntimeInputs = lib.unique (
    [
      pkgs.coreutils
      pkgs.gnugrep
      pkgs.gnused
      pkgs.curl
      netcatPkg
    ]
    ++ serviceCheckRuntimeInputs
  );

  envNames = runtime.env.names;
  envPattern =
    if envNames == [ ] then runtime.env.default else builtins.concatStringsSep "|" envNames;
  isolationEnvValues = if envNames == [ ] then [ runtime.env.default ] else envNames;
  isolationSlotVar = runtime.slot.var;
  isolationEnvVar = runtime.env.var;
  isolationMaxSlot = runtime.slot.max;
  isolationRunTaskId = cfg.testIsolation.runTaskId;
  isolationValidateTaskId = cfg.testIsolation.validateTaskId;
  renderShellArray =
    name: values:
    ''
      ${name}=()
    ''
    + builtins.concatStringsSep "\n" (
      map (value: "${name}+=(${lib.escapeShellArg (toString value)})") values
    )
    + "\n";
  renderEnvValue = value: if builtins.isString value then value else builtins.toJSON value;
  isolationSlotsShell = renderShellArray "isolation_slots" cfg.testIsolation.slots;
  isolationEnvsShell = renderShellArray "isolation_envs" cfg.testIsolation.envs;
  isolationRunArgsShell = renderShellArray "run_args" cfg.testIsolation.runArgs;
  isolationRunEnvEntriesShell = renderShellArray "run_env_entries" (
    map (key: "${key}\t${renderEnvValue cfg.testIsolation.runEnv.${key}}") (
      builtins.sort builtins.lessThan (builtins.attrNames cfg.testIsolation.runEnv)
    )
  );

  envOffsetCase = builtins.concatStringsSep "\n" (
    map (
      envName: "    ${envName}) env_offset=${toString (runtime.env.offsets.${envName} or 0)} ;;"
    ) envNames
  );
  slotEnvPrelude = ''
        ${shellCommon}
        slot_var=${lib.escapeShellArg runtime.slot.var}
        env_var=${lib.escapeShellArg runtime.env.var}
        slot_default=${toString runtime.slot.default}
        env_default=${lib.escapeShellArg runtime.env.default}

        slot_value="''${!slot_var:-$slot_default}"
        env_value="''${!env_var:-$env_default}"

        if ! [[ "$slot_value" =~ ^[0-9]+$ ]]; then
          nixfied_exit_precondition "$slot_var must be an integer"
        fi

        case "$env_value" in
    ${envOffsetCase}
          *)
            nixfied_exit_precondition "unsupported $env_var '$env_value'"
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

  probeHelpers = import ./operations/probes.nix {
    inherit
      lib
      pkgs
      runtime
      services
      serviceNames
      resolvePortBase
      shellCommon
      slotEnvPrelude
      skipPolicy
      serviceConfigLib
      ;
  };

  inherit (probeHelpers) serviceSelectionContractArgs;
  inherit (probeHelpers) mkProbeScript;

  mkTask =
    {
      id,
      summary,
      description,
      command,
      runtimeInputs ? [ ],
      contractArgs ? [ ],
      launcher ? null,
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
        inherit command;
      };
      commandApi = {
        version = 2;
        commandClass = "typed";
        inherit summary;
        details = description;
        usage = [ ];
        examples = [ ];
        category = "ops";
        args = contractArgs;
        env = [ ];
        outputs = {
          mode = "text";
          channels = "stdout";
          keys = [ ];
        };
        behavior = {
          idempotent = true;
          effects = [ "none" ];
          timeoutSec = 0;
        };
        errors.codes = builtins.removeAttrs exitCodes [
          "canceled"
          "unavailable"
          "timeout"
        ];
      };
      launcher = if launcher == null then { } else launcher;
      runtime = {
        slotEnv = "optional";
        workdir = "projectRoot";
        hermetic = true;
        inherit runtimeInputs;
        passThroughEnv = [
          runtime.env.var
          runtime.slot.var
        ];
        references = {
          taskIds = [ ];
          workflowIds = [ ];
        };
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
    };

  validateScript = ''
    set -euo pipefail
    ${shellCommon}

    slot_var=${lib.escapeShellArg runtime.slot.var}
    env_var=${lib.escapeShellArg runtime.env.var}

    slot_default=${toString runtime.slot.default}
    slot_max=${toString runtime.slot.max}
    env_default=${lib.escapeShellArg runtime.env.default}

    slot_value="''${!slot_var:-$slot_default}"
    env_value="''${!env_var:-$env_default}"

    if ! [[ "$slot_value" =~ ^[0-9]+$ ]]; then
      nixfied_exit_precondition "$slot_var must be an integer"
    fi

    if [ "$slot_value" -gt "$slot_max" ]; then
      nixfied_exit_precondition "$slot_var exceeds max slot ($slot_max)"
    fi

    case "$env_value" in
      ${envPattern}) ;;
      *)
        nixfied_exit_precondition "unsupported $env_var '$env_value'"
        ;;
    esac

    echo "OK: environment is valid (''${env_var}=$env_value ''${slot_var}=$slot_value)"
  '';

  portsScript = ''
        set -euo pipefail
        ${shellCommon}

        slot_var=${lib.escapeShellArg runtime.slot.var}
        env_var=${lib.escapeShellArg runtime.env.var}
        slot_default=${toString runtime.slot.default}
        env_default=${lib.escapeShellArg runtime.env.default}

        slot_value="''${!slot_var:-$slot_default}"
        env_value="''${!env_var:-$env_default}"

        case "$env_value" in
    ${envOffsetCase}
          *)
            nixfied_exit_precondition "unsupported $env_var '$env_value'"
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
    emptyMessage = "SKIP: no enabled services for health checks";
    successMessage = "OK: health checks passed";
  };

  readyScript = mkProbeScript {
    mode = "ready";
    emptyMessage = "SKIP: no enabled services for readiness checks";
    successMessage = "OK: readiness checks passed";
  };

  isolationScript = testIsolationRuntime.mkIsolationScript {
    slotVar = isolationSlotVar;
    envVar = isolationEnvVar;
    slotMax = isolationMaxSlot;
    maxParallelDefault = cfg.testIsolation.maxParallel;
    logsRootBase = cfg.testIsolation.logsDir;
    runTaskId = isolationRunTaskId;
    validateTaskId = isolationValidateTaskId;
    keepLogsSuccess = cfg.testIsolation.keepLogsOnSuccess;
    keepLogsFailure = cfg.testIsolation.keepLogsOnFailure;
    slotsShell = isolationSlotsShell;
    envsShell = isolationEnvsShell;
    runArgsShell = isolationRunArgsShell;
    runEnvEntriesShell = isolationRunEnvEntriesShell;
    inherit envPattern;
  };
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

    testIsolation = {
      enable = lib.mkOption {
        type = lib.types.bool;
        default = true;
      };

      slots = lib.mkOption {
        type = lib.types.listOf lib.types.int;
        default = [ runtime.slot.default ];
      };

      envs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = runtime.env.names;
      };

      logsDir = lib.mkOption {
        type = lib.types.str;
        default = "/tmp/${config.nixfied.identity.projectId}-isolation";
      };

      keepLogsOnSuccess = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };

      keepLogsOnFailure = lib.mkOption {
        type = lib.types.bool;
        default = true;
      };

      maxParallel = lib.mkOption {
        type = lib.types.ints.positive;
        default = 4;
      };

      runTaskId = lib.mkOption {
        type = lib.types.str;
        default = "task.ci";
      };

      runApp = lib.mkOption {
        type = lib.types.str;
        default = "ci";
      };

      runArgs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "--summary" ];
      };

      validateApp = lib.mkOption {
        type = lib.types.str;
        default = "validate-env";
      };

      validateTaskId = lib.mkOption {
        type = lib.types.str;
        default = "task.ops.validate-env";
      };

      runEnv = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.oneOf [
            lib.types.str
            lib.types.int
            lib.types.bool
          ]
        );
        default = { };
      };
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
        summary = "Validate slot/env settings";
        description = "Validates PROJECT_ENV and NIX_ENV values against model runtime constraints.";
        command = validateScript;
        launcher = {
          enable = true;
          appId = "validate-env";
          category = "ops";
          ownerFile = "nixfied/modules/operations.nix";
          usage = [ "nix run .#validate-env" ];
        };
      };
    })

    (lib.mkIf (cfg.enable && cfg.testIsolation.enable) {
      nixfied.tasks."test-isolation" =
        mkTask {
          id = "task.ops.test-isolation";
          summary = "Run isolation checks";
          description = "Runs deterministic isolation smoke checks from model metadata.";
          command = isolationScript;
          contractArgs = testIsolationContractArgs;
          runtimeInputs = [ pkgs.coreutils ];
          launcher = {
            enable = true;
            appId = "test-isolation";
            category = "ops";
            ownerFile = "nixfied/modules/operations.nix";
            usage = [ "nix run .#test-isolation" ];
          };
        }
        // {
          runtime.references.taskIds = [
            isolationValidateTaskId
            isolationRunTaskId
          ];
        };
    })

    (lib.mkIf (cfg.enable && cfg.ports.enable) {
      nixfied.tasks."ports" = mkTask {
        id = "task.ops.ports";
        summary = "Print model-derived port assignments";
        description = "Prints computed per-slot/per-env ports from compiled runtime data.";
        command = portsScript;
        launcher = {
          enable = true;
          appId = "ports";
          category = "ops";
          ownerFile = "nixfied/modules/operations.nix";
          usage = [ "nix run .#ports" ];
        };
      };
    })

    (lib.mkIf (cfg.enable && cfg.checkPorts.enable) {
      nixfied.tasks."check-ports" = mkTask {
        id = "task.ops.check-ports";
        summary = "Check model-derived port availability";
        description = "Checks if computed per-slot/per-env ports are listening or free.";
        command = checkPortsScript;
        runtimeInputs = [
          pkgs.coreutils
          pkgs.gnugrep
          pkgs.gnused
          (pkgs.lsof or pkgs.coreutils)
        ];
        launcher = {
          enable = true;
          appId = "check-ports";
          category = "ops";
          ownerFile = "nixfied/modules/operations.nix";
          usage = [ "nix run .#check-ports" ];
        };
      };
    })

    (lib.mkIf (cfg.enable && cfg.health.enable) {
      nixfied.tasks."health" = mkTask {
        id = "task.ops.health";
        summary = "Run service health checks";
        description = ''
          Runs health checks for selected enabled services declared under nixfied.services.
          Optional selectors: --service <name|all> and --source <key>.
          One service selector is accepted per invocation.
        '';
        command = healthScript;
        runtimeInputs = serviceProbeRuntimeInputs;
        contractArgs = serviceSelectionContractArgs;
        launcher = {
          enable = true;
          appId = "health";
          category = "ops";
          ownerFile = "nixfied/modules/operations.nix";
          usage = [
            "nix run .#health"
            "nix run .#health -- --service postgres"
            "nix run .#health -- --service helios --source real"
          ];
          examples = [
            "nix run .#health -- --service postgres"
            "nix run .#health -- --service helios --source real"
          ];
        };
      };
    })

    (lib.mkIf (cfg.enable && cfg.ready.enable) {
      nixfied.tasks."ready" = mkTask {
        id = "task.ops.ready";
        summary = "Run service readiness checks";
        description = ''
          Runs readiness checks for selected enabled services declared under nixfied.services.
          Optional selectors: --service <name|all> and --source <key>.
          One service selector is accepted per invocation.
        '';
        command = readyScript;
        runtimeInputs = serviceProbeRuntimeInputs;
        contractArgs = serviceSelectionContractArgs;
        launcher = {
          enable = true;
          appId = "ready";
          category = "ops";
          ownerFile = "nixfied/modules/operations.nix";
          usage = [
            "nix run .#ready"
            "nix run .#ready -- --service postgres"
            "nix run .#ready -- --service helios --source real"
          ];
          examples = [
            "nix run .#ready -- --service postgres"
            "nix run .#ready -- --service helios --source real"
          ];
        };
      };
    })
  ];
}
