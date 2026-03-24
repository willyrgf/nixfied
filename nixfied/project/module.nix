{
  lib,
  pkgs,
  frameworkSourceRevision ? "unknown",
  ...
}:
let
  conf = import ./conf.nix { inherit pkgs; };
  exitCodes = import ../framework/core/exit-codes.nix;
  project = conf.project;
  envNames = builtins.attrNames conf.envs;
  envOffsets = lib.mapAttrs (_: value: value.offset or 0) conf.envs;
  normalizeSourceKeys = sources: builtins.sort builtins.lessThan (builtins.attrNames sources);
  normalizePostgresEnvConfigs = lib.mapAttrs (
    _: envCfg: {
      extraConfig = envCfg.extraConfig or "";
    }
  );

  commonRuntimeInputs = [
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.gnugrep
  ];

  nixFormatterPkg = if pkgs ? nixfmt then pkgs.nixfmt else pkgs.nixfmt-rfc-style;
  nixChecksPkg = import ../framework/core/mkNixChecks.nix {
    inherit
      pkgs
      lib
      ;
  } { };

  loggingContractArgs = [
    {
      name = "log-level";
      kind = "option";
      long = "--log-level";
      type = "enum";
      values = [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      description = "Override LOG_LEVEL for this run.";
    }
    {
      name = "output-mode";
      kind = "option";
      long = "--output-mode";
      type = "enum";
      values = [
        "stdout"
        "logs"
        "both"
      ];
      description = "Override OUTPUT_MODE for this run.";
    }
  ];

  nixChecksContractArgs = [
    {
      name = "mode";
      kind = "option";
      long = "--mode";
      type = "enum";
      values = [
        "quick"
        "full"
      ];
      description = "Check profile to run.";
    }
    {
      name = "quick";
      kind = "flag";
      long = "--quick";
      description = "Alias for --mode quick.";
    }
    {
      name = "full";
      kind = "flag";
      long = "--full";
      description = "Alias for --mode full.";
    }
  ];

  mergeLoggingContractArgs =
    contractArgs:
    let
      argIdentity =
        arg:
        let
          argName = if arg ? name then arg.name else "";
          argLong = if (arg ? long) && arg.long != null then arg.long else "";
        in
        "${argName}|${argLong}";
      existing = map argIdentity contractArgs;
    in
    contractArgs ++ lib.filter (arg: !(builtins.elem (argIdentity arg) existing)) loggingContractArgs;

  defaultTaskPassThroughEnv = [
    project.envVar
    project.slotVar
    "CI_MAX_WORKERS"
    "NIXFIED_CI_MAX_WORKERS"
    "LOG_LEVEL"
    "NIXFIED_LOG_LEVEL"
    "OUTPUT_MODE"
    "NIXFIED_OUTPUT_MODE"
    "RUST_LOG"
    "MFM_LOG"
    "MFM_TEST_LOG"
    "MFM_TEST_LOG_FILTER"
  ];

  taskCacheDirPreHook = ''
    if [ -n "''${CARGO_TARGET_DIR:-}" ]; then
      echo "INFO: using existing CARGO_TARGET_DIR=$CARGO_TARGET_DIR"
    else
      mfm_cache_fallback_hash() {
        local input="$1"
        local checksum=""

        if command -v sha256sum >/dev/null 2>&1; then
          checksum="$(printf '%s' "$input" | sha256sum)"
          checksum="''${checksum%% *}"
        elif command -v cksum >/dev/null 2>&1; then
          checksum="$(printf '%s' "$input" | cksum)"
          checksum="''${checksum%% *}"
        fi
        if [ -z "$checksum" ]; then
          checksum="$(( ''${#input} ))"
        fi
        printf '%s' "$checksum"
      }

      mfm_cache_sanitize_component() {
        local input="$1"
        local fallback="$2"
        local max_len="$3"
        local component=""
        local suffix=""
        local safe_len=""

        component="$(printf '%s' "$input" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9._-' '-')"
        component="''${component:-$fallback}"

        while [ -n "$component" ] && [ "''${component:0:1}" = "-" ]; do
          component="''${component#-}"
        done
        while [ -n "$component" ] && [ "''${component: -1}" = "-" ]; do
          component="''${component%-}"
        done

        if [ -z "$component" ]; then
          component="$fallback"
        fi

        safe_len="$max_len"
        if [ "''${#component}" -gt "$safe_len" ]; then
          suffix="$(mfm_cache_fallback_hash "$input")"
          component="$(printf '%s' "''${component}" | cut -c 1-"$((safe_len - 11))")-$(printf '%s' "''${suffix}" | cut -c 1-10)"
        fi

        printf '%s' "$component"
      }

      run_id_component="$(mfm_cache_sanitize_component "''${NIXFIED_ATTEMPT_ID:-''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-''${NIXFIED_ORCHESTRATOR_RUN_ID:-''${NIXFIED_RUN_ID:-''${NIX_ENV:-0}}}}}" "run" 42)"
      workflow_id_component="$(mfm_cache_sanitize_component "''${NIXFIED_PARENT_WORKFLOW_ID:-''${NIXFIED_ORCHESTRATOR_WORKFLOW_ID:-workflow}}" "workflow" 42)"
      task_id_component="$(mfm_cache_sanitize_component "''${NIXFIED_TASK_ID:-orchestrator}" "orchestrator" 42)"

      task_cache_key="''${run_id_component}/''${workflow_id_component}/''${task_id_component}"
      task_cache_dir="''${TMPDIR:-/tmp}/mfm-ci-target/''${task_cache_key}"
      if [ "''${#task_cache_dir}" -gt 220 ]; then
        task_cache_key="$(mfm_cache_fallback_hash "''${task_cache_key}")"
        task_cache_dir="''${TMPDIR:-/tmp}/mfm-ci-target/''${task_cache_key}"
        if [ "''${#task_cache_dir}" -gt 220 ]; then
          task_cache_dir="''${TMPDIR:-/tmp}/mfm-ci-target/$run_id_component"
        fi
      fi

      export NIXFIED_TASK_CACHE_KEY="''${task_cache_key}"
      export CARGO_TARGET_DIR="''${task_cache_dir}"
      mkdir -p "$CARGO_TARGET_DIR"
      echo "INFO: task cache key=$NIXFIED_TASK_CACHE_KEY"
      echo "INFO: task cache dir=$CARGO_TARGET_DIR"
    fi
  '';

  mkCommandTask =
    {
      id,
      summary,
      description ? "",
      command ? "",
      kind ? "command",
      tags ? [ ],
      runtimeInputs ? commonRuntimeInputs,
      preHooks ? { },
      postHooks ? { },
      workflowId ? null,
      runner ? null,
      contractArgs ? [ ],
      logging ? { },
      passThroughEnv ? defaultTaskPassThroughEnv,
      allowSensitivePassThrough ? false,
      launcher ? null,
      ownerFile ? "nixfied/project/tasks.nix",
    }:
    let
      taskSupportsHooks =
        if runner != null then ((runner.type or "") == "shell") else (workflowId == null);
    in
    {
      inherit
        id
        kind
        summary
        description
        tags
        ;

      runner =
        if runner != null then
          runner
        else if workflowId == null then
          {
            type = "shell";
            command = command;
          }
        else
          {
            type = "workflowRef";
            workflowId = workflowId;
      };

      commandApi = {
        version = 2;
        commandClass = "typed";
        summary = summary;
        details = description;
        usage = [ ];
        examples = [ ];
        category = kind;
        args = mergeLoggingContractArgs contractArgs;
        env = [ ];
        outputs = {
          mode = "text";
          channels = "stdout";
          keys = [ ];
        };
        behavior = {
          idempotent = false;
          effects = [ "writes-state" ];
          timeoutSec = 0;
        };
        errors.codes = builtins.removeAttrs exitCodes [
          "canceled"
          "unavailable"
          "timeout"
        ];
      };

      launcher =
        if launcher == null then
          { }
        else
          launcher;

      runtime = {
        slotEnv = "optional";
        workdir = "projectRoot";
        hermetic = true;
        runtimeInputs = runtimeInputs;
        passThroughEnv = passThroughEnv;
        allowSensitivePassThrough = allowSensitivePassThrough;
        logging = {
          levelDefault = logging.levelDefault or null;
          outputDefault = logging.outputDefault or null;
        };
        env = { };
        umask = "022";
        locale = "C.UTF-8";
        timezone = "UTC";
        preHooks =
          if taskSupportsHooks then
            (
              preHooks
              // {
                "framework.task-cache-dir" = {
                  command = taskCacheDirPreHook;
                };
              }
            )
          else
            preHooks;
        postHooks = postHooks;
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

  mkTaskLauncher =
    {
      appId,
      summary ? "",
      description ? "",
      usage ? [ "nix run .#${appId}" ],
      examples ? [ ],
      category ? "core",
      ownerFile ? "nixfied/project/tasks.nix",
    }:
    {
      enable = true;
      appId = appId;
      inherit
        summary
        description
        usage
        examples
        category
        ownerFile
        ;
    };

  frameworkInstallPreset = import ../framework/presets/install.nix {
    inherit
      mkCommandTask
      mkTaskLauncher
      pkgs
      frameworkSourceRevision
      ;
  };

  frameworkTestPreset = import ../framework/presets/framework-test.nix {
    inherit
      lib
      pkgs
      conf
      mkCommandTask
      mkTaskLauncher
      ;
  };

  frameworkSelfhostPreset = import ../framework/presets/selfhost.nix {
    inherit
      mkCommandTask
      commonRuntimeInputs
      ;
  };

  projectRuntimeModule = import ./runtime.nix {
    inherit
      conf
      project
      envNames
      envOffsets
      nixChecksPkg
      ;
  };

  projectServicesModule = import ./services.nix {
    inherit
      conf
      normalizeSourceKeys
      normalizePostgresEnvConfigs
      ;
  };

  projectTasksModule = import ./tasks.nix {
    inherit
      lib
      mkCommandTask
      mkTaskLauncher
      commonRuntimeInputs
      nixChecksPkg
      nixChecksContractArgs
      defaultTaskPassThroughEnv
      nixFormatterPkg
      frameworkInstallPreset
      frameworkTestPreset
      frameworkSelfhostPreset
      ;
  };

  workspaceStatePolicyPreset = import ../framework/presets/state-policies/workspace-scoped.nix;

  projectWorkflowsModule = import ./workflows.nix { inherit frameworkSelfhostPreset; };
in
{
  imports = [
    ../modules/profiles/webapp.nix
    workspaceStatePolicyPreset
    projectRuntimeModule
    projectServicesModule
    projectTasksModule
    projectWorkflowsModule
  ];

  config = { };
}
