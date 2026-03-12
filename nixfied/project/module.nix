{
  lib,
  pkgs,
  projectRoot,
  frameworkSourceRevision ? "unknown",
  ...
}:
let
  conf = import ./conf.nix { inherit pkgs; };
  plainShellLogging = import ../lib/plain-shell-logging.nix;
  project = conf.project;
  workspaceId = builtins.substring 0 12 (builtins.hashString "sha256" (toString projectRoot));
  workspaceRuntimeRoot = "/tmp/nixfied-runtime/${project.id}/${workspaceId}";
  legacyRuntimeBases = [
    "\${XDG_DATA_HOME:-$HOME/.local/share}/${project.id}"
    "/tmp/nixfied-runtime/${project.id}/runtime"
  ];
  legacyRegistryRoots = [
    "/tmp/nixfied-runtime/${project.id}"
    "/tmp/nixfied-runtime/${project.id}/registry"
  ];
  resolvedRuntimeBase =
    if builtins.elem conf.directories.base legacyRuntimeBases then
      "${workspaceRuntimeRoot}/runtime"
    else
      conf.directories.base;
  resolvedRegistryRoot =
    if builtins.elem conf.process.registryRoot legacyRegistryRoots then
      "${workspaceRuntimeRoot}/registry"
    else
      conf.process.registryRoot;
  resolvedArtifactsRoot = "/tmp/ci-artifacts/${project.id}/${workspaceId}";
  frameworkTestMaxParallelShardsRaw = conf.frameworkTest.maxParallelShards or "auto";
  frameworkTestMaxParallelShards =
    if builtins.isInt frameworkTestMaxParallelShardsRaw then
      toString frameworkTestMaxParallelShardsRaw
    else if builtins.isString frameworkTestMaxParallelShardsRaw then
      frameworkTestMaxParallelShardsRaw
    else
      throw "ERROR: frameworkTest.maxParallelShards must be \"auto\" or a positive integer";

  envNames = builtins.attrNames conf.envs;
  envOffsets = lib.mapAttrs (_: value: value.offset or 0) conf.envs;
  normalizeSourceKeys = sources: builtins.sort builtins.lessThan (builtins.attrNames sources);
  normalizePostgresEnvConfigs = lib.mapAttrs (
    _: envCfg: {
      extraConfig = envCfg.extraConfig or "";
    }
  );

  thinWrapperFlake = import ../install/wrapper-flake.nix {
    frameworkInput = "github:willyrgf/nixfied/dev";
  };

  vendoredWrapperFlake = import ../install/wrapper-flake.nix {
    vendorPath = "./nixfied";
  };

  vendoredMetadata = ''
    Vendored Framework
    ==================

    This repository vendors the Nixfied framework under `nixfied/`.

    Framework source revision (install/upgrade):
    - ${frameworkSourceRevision}

    Framework source revision workflow:
    - initialized via `framework::install`
    - upgraded via `framework::upgrade` (preserves `nixfied/project/` and `nixfied/local/` by default)

    Framework-owned paths:
    - `flake.nix`, `flake.lock`
    - `nixfied/.framework/`

    User-owned customization paths:
    - `nixfied/project/` (primary command/task/workflow customization surface)
    - `nixfied/local/` (optional extensions)

    Prefer editing `nixfied/project/` and `nixfied/local/` over direct framework internals.
  '';

  commonRuntimeInputs = [
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.gnugrep
  ];

  nixFormatterPkg = if pkgs ? nixfmt then pkgs.nixfmt else pkgs.nixfmt-rfc-style;
  nixChecksPkg = import ../lib/mkNixChecks.nix {
    inherit
      pkgs
      lib
      ;
  } { };

  frameworkInstallRuntimeInputs = [
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.rsync
  ];

  frameworkInstallContractArgs = [
    {
      name = "vendor";
      kind = "flag";
      long = "--vendor";
      description = "Generate a vendored wrapper flake.";
    }
    {
      name = "target";
      kind = "option";
      long = "--target";
      type = "string";
      description = "Output directory for generated wrapper.";
    }
    {
      name = "upgrade";
      kind = "flag";
      long = "--upgrade";
      description = "Upgrade vendored framework files in-place and preserve nixfied/project + nixfied/local.";
    }
    {
      name = "reset-project";
      kind = "flag";
      long = "--reset-project";
      description = "When vendoring, overwrite nixfied/project.";
    }
    {
      name = "reset-local";
      kind = "flag";
      long = "--reset-local";
      description = "When vendoring, overwrite nixfied/local.";
    }
  ];

  frameworkUpgradeContractArgs = [
    {
      name = "vendor";
      kind = "flag";
      long = "--vendor";
      description = "Generate a vendored wrapper flake (default for framework::upgrade).";
    }
    {
      name = "target";
      kind = "option";
      long = "--target";
      type = "string";
      description = "Output directory for generated wrapper.";
    }
    {
      name = "reset-project";
      kind = "flag";
      long = "--reset-project";
      description = "When vendoring, overwrite nixfied/project.";
    }
    {
      name = "reset-local";
      kind = "flag";
      long = "--reset-local";
      description = "When vendoring, overwrite nixfied/local.";
    }
  ];

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

  mkFrameworkInstallCommand =
    {
      upgradeDefault ? false,
    }:
    ''
            set -euo pipefail

            source_root="${builtins.toString ../.}"
            repo_root="${builtins.toString ../../.}"
            target="."
            vendor=${if upgradeDefault then "1" else "0"}
            upgrade=${if upgradeDefault then "1" else "0"}
            reset_project=0
            reset_local=0

            usage() {
              cat <<'EOF'
      ${
        if upgradeDefault then
          ''
            Usage:
              nix run .#framework::upgrade -- --target .
              nix run .#framework::upgrade -- --target . --reset-project
              nix run .#framework::upgrade -- --target . --reset-local

            Upgrade vendored wrapper in-place while preserving nixfied/project and nixfied/local by default.

            Options:
              --vendor          Generate a vendored wrapper flake (default for framework::upgrade).
              --target <path>   Output directory for generated wrapper.
              --reset-project   When vendoring, overwrite nixfied/project.
              --reset-local     When vendoring, overwrite nixfied/local.
              --help, -h        Show this help.
          ''
        else
          ''
            Usage:
              nix run .#framework::install
              nix run .#framework::install -- --vendor
              nix run .#framework::install -- --vendor --target .
              nix run .#framework::install -- --vendor --upgrade --target .

            Install a thin wrapper flake by default, or a vendored wrapper with --vendor.

            Options:
              --vendor          Generate a vendored wrapper flake.
              --target <path>   Output directory for generated wrapper.
              --upgrade         Upgrade vendored framework files in-place and preserve nixfied/project + nixfied/local.
              --reset-project   When vendoring, overwrite nixfied/project.
              --reset-local     When vendoring, overwrite nixfied/local.
              --help, -h        Show this help.
          ''
      }
      EOF
            }

            ${plainShellLogging {
              includeWarn = false;
              includeSkip = false;
              errorToStderr = true;
            }}

            while [ "$#" -gt 0 ]; do
              case "$1" in
                --vendor)
                  vendor=1
                  shift
                  ;;
                --upgrade)
                  upgrade=1
                  vendor=1
                  shift
                  ;;
                --reset-project)
                  reset_project=1
                  shift
                  ;;
                --reset-local)
                  reset_local=1
                  shift
                  ;;
                --target)
                  if [ "$#" -lt 2 ]; then
                    log_error "--target requires a value"
                    exit 2
                  fi
                  target="$2"
                  shift 2
                  ;;
                --help|-h)
                  usage
                  exit 0
                  ;;
                --)
                  shift
                  break
                  ;;
                *)
                  log_error "unknown argument '$1'"
                  exit 2
                  ;;
              esac
            done

            if [ "$#" -gt 0 ]; then
              log_error "unexpected positional arguments: $*"
              exit 2
            fi

            if [ "$vendor" -eq 0 ] && { [ "$reset_project" -eq 1 ] || [ "$reset_local" -eq 1 ]; }; then
              log_error "--reset-project/--reset-local require --vendor"
              exit 2
            fi

            mkdir -p "$target"

            if [ "$vendor" -eq 1 ]; then
              stage_dir="$(mktemp -d)"
              cleanup_stage() {
                rm -rf "$stage_dir"
              }
              trap cleanup_stage EXIT

              mkdir -p "$stage_dir/nixfied"
              cp -R "$source_root/." "$stage_dir/nixfied"
              if [ -f "$repo_root/README.md" ]; then
                cp "$repo_root/README.md" "$stage_dir/nixfied/README.md"
              fi
              chmod -R u+w "$stage_dir/nixfied" 2>/dev/null || true
              rm -rf "$stage_dir/nixfied/.git"
              rm -f "$stage_dir/nixfied/result"
              rm -f "$stage_dir/nixfied/.framework/.workspace"

              preserve_project=0
              preserve_local=0
              if [ -d "$target/nixfied/project" ] && [ "$reset_project" -eq 0 ]; then
                preserve_project=1
              fi
              if [ -d "$target/nixfied/local" ] && [ "$reset_local" -eq 0 ]; then
                preserve_local=1
              fi

              mkdir -p "$target/nixfied"
              chmod -R u+w "$target/nixfied" 2>/dev/null || true

              preserve_msg=""
              rsync_args=(-a --delete --chmod=Du+w,Fu+w)
              if [ "$preserve_project" -eq 1 ]; then
                rsync_args+=(--exclude='/project/')
                preserve_msg="nixfied/project/"
              fi
              if [ "$preserve_local" -eq 1 ]; then
                rsync_args+=(--exclude='/local/')
                if [ -n "$preserve_msg" ]; then
                  preserve_msg="$preserve_msg and nixfied/local/"
                else
                  preserve_msg="nixfied/local/"
                fi
              fi
              if [ -n "$preserve_msg" ]; then
                log_info "upgrading vendored wrapper (preserving $preserve_msg)"
              fi

              ${pkgs.rsync}/bin/rsync "''${rsync_args[@]}" "$stage_dir/nixfied/" "$target/nixfied/"
              rm -f "$target/nixfied/.framework/.workspace"

              cat > "$target/nixfied/VENDORED.txt" <<'NIXFIED_VENDORED'
      ${vendoredMetadata}
      NIXFIED_VENDORED

              cat > "$target/flake.nix" <<'NIXFIED_WRAPPER'
      ${vendoredWrapperFlake}
      NIXFIED_WRAPPER

              if [ "$upgrade" -eq 1 ] || [ -n "$preserve_msg" ]; then
                log_ok "vendored wrapper upgraded at $target/flake.nix"
              else
                log_ok "vendored wrapper flake generated at $target/flake.nix"
              fi
            else
              cat > "$target/flake.nix" <<'NIXFIED_WRAPPER'
      ${thinWrapperFlake}
      NIXFIED_WRAPPER
              if [ "$upgrade" -eq 1 ]; then
                log_ok "thin wrapper flake updated at $target/flake.nix"
              else
                log_ok "thin wrapper flake generated at $target/flake.nix"
              fi
            fi
    '';

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

  mkCommandTask =
    {
      id,
      appName,
      summary,
      description ? "",
      command ? "",
      kind ? "command",
      tags ? [ ],
      usage ? [ ],
      examples ? [ ],
      runtimeInputs ? commonRuntimeInputs,
      preHooks ? { },
      postHooks ? { },
      workflowId ? null,
      runner ? null,
      contractArgs ? [ ],
      logging ? { },
      passThroughEnv ? defaultTaskPassThroughEnv,
      allowSensitivePassThrough ? false,
      ownerFile ? "nixfied/project/module.nix",
    }:
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

      contract = {
        version = 1;
        input = {
          args = {
            parser = "typed";
            allowUnknown = false;
            spec = mergeLoggingContractArgs contractArgs;
          };
          env = {
            schemaRef = "runtimePrimitives";
            extra = [ ];
          };
        };
        output = {
          format = "text";
          channels = "stdout";
          keys = [ ];
        };
        behavior = {
          idempotent = false;
          effects = [ "writes-state" ];
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
        preHooks = preHooks;
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

      ui.app = {
        expose = true;
        name = appName;
        category = "core";
        usage = usage;
        examples = examples;
        ownerFile = ownerFile;
      };
    };

  frameworkInstallPreset = import ../framework/presets/install.nix {
    inherit
      mkCommandTask
      frameworkInstallRuntimeInputs
      frameworkInstallContractArgs
      frameworkUpgradeContractArgs
      mkFrameworkInstallCommand
      ;
  };

  frameworkTestPreset = import ../framework/presets/framework-test.nix {
    inherit
      lib
      pkgs
      mkCommandTask
      frameworkTestMaxParallelShards
      plainShellLogging
      ;
  };

  frameworkSelfhostPreset = import ../framework/presets/selfhost.nix {
    inherit
      mkCommandTask
      commonRuntimeInputs
      ;
  };
in
{
  imports = [
    ../modules/profiles/webapp.nix
  ];

  config = {
    nixfied = {
      identity = {
        projectId = project.id;
        projectName = project.name;
        description = project.description;
      };

      runtime = {
        slot = {
          var = project.slotVar;
          default = conf.slots.default;
          max = conf.slots.max;
          stride = conf.slots.stride;
        };

        env = {
          var = project.envVar;
          names = envNames;
          offsets = envOffsets;
          default = "dev";
        };

        logging = {
          levelDefault = conf.logging.level;
          outputDefault = conf.logging.output;
        };

        ports = conf.ports;
        directories.base = resolvedRuntimeBase;
        ephemeral = {
          copyMode = conf.ephemeral.copyMode or "nix-source";
          includeUntracked = conf.ephemeral.includeUntracked or false;
          excludePatterns =
            conf.ephemeral.excludePatterns or [
              ".git"
              "node_modules"
              ".next"
              "dist"
              ".turbo"
              ".cache"
              "result"
              "result-*"
              "*.log"
              "test-results"
              "coverage"
            ];
          extraDirs = conf.ephemeral.extraDirs or [ ];
          keepFailures = conf.ephemeral.keepFailures or true;
          maxFailedRoots = conf.ephemeral.maxFailedRoots or 8;
          maxFailedRootAgeHours = conf.ephemeral.maxFailedRootAgeHours or 72;
          maxCopyBytes = conf.ephemeral.maxCopyBytes or 0;
          minFreeBytesAfterCopy = conf.ephemeral.minFreeBytesAfterCopy or 0;
          envFileMode = conf.ephemeral.envFileMode or "disabled";
          envFilePath = conf.ephemeral.envFilePath or ".env";
        };
      };

      state = {
        workspaceId = workspaceId;
        registryRoot = resolvedRegistryRoot;
        artifactsRoot = resolvedArtifactsRoot;
      };

      tooling = {
        runtimePackages = conf.tooling.runtimePackages;
        devShellPackages = conf.tooling.devShellPackages;
        devShellHook = conf.tooling.devShellHook;
      };

      packages = {
        "nix-checks" = nixChecksPkg;
      };

      services = {
        postgres = {
          enable = conf.services.postgres.enable or false;
          database = conf.services.postgres.database or "app";
          testDatabase = conf.services.postgres.testDatabase or "app_test";
          portKey = conf.services.postgres.ports.primary or "postgres";
          dataDirName = conf.services.postgres.dataDirName or "postgres";
          extensions = conf.services.postgres.extensions or [ ];
          extraConfig = conf.services.postgres.extraConfig or "";
          envConfigs = normalizePostgresEnvConfigs (conf.services.postgres.envConfigs or { });
          migrations = {
            dir = conf.services.postgres.migrations.dir or "migrations";
            command = conf.services.postgres.migrations.command or "";
            sourceDatabase = conf.services.postgres.migrations.sourceDatabase or null;
          };
          sources = conf.services.postgres.sources or { };
          sourceKeys = normalizeSourceKeys (conf.services.postgres.sources or { });
          defaultSource = conf.services.postgres.defaultSource or "";
        };

        nginx = {
          enable = conf.services.nginx.enable or false;
          portKeyHttp = conf.services.nginx.ports.http or "http";
          portKeyHttps = conf.services.nginx.ports.https or "https";
          dataDirName = conf.services.nginx.dataDirName or "nginx";
          sources = conf.services.nginx.sources or { };
          sourceKeys = normalizeSourceKeys (conf.services.nginx.sources or { });
          defaultSource = conf.services.nginx.defaultSource or "";
        };

        minio = {
          enable = conf.services.minio.enable or false;
          portKeyApi = conf.services.minio.ports.api or "minioApi";
          portKeyConsole = conf.services.minio.ports.console or "minioConsole";
          dataDirName = conf.services.minio.dataDirName or "minio";
          rootUser = conf.services.minio.rootUser or "minioadmin";
          rootPassword = conf.services.minio.rootPassword or "minioadmin";
          browser = conf.services.minio.browser or true;
          sources = conf.services.minio.sources or { };
          sourceKeys = normalizeSourceKeys (conf.services.minio.sources or { });
          defaultSource = conf.services.minio.defaultSource or "";
        };

        reth = {
          enable = conf.services.reth.enable or false;
          portKeyHttp = conf.services.reth.ports.http or "rethHttp";
          portKeyWs = conf.services.reth.ports.ws or "rethWs";
          portKeyAuth = conf.services.reth.ports.auth or "rethAuth";
          dataDirName = conf.services.reth.dataDirName or "reth";
          network = conf.services.reth.network or "local";
          devMode = conf.services.reth.devMode or false;
          extraArgs = conf.services.reth.extraArgs or [ ];
          sources = conf.services.reth.sources or { };
          sourceKeys = normalizeSourceKeys (conf.services.reth.sources or { });
          defaultSource = conf.services.reth.defaultSource or "";
        };

        helios = {
          enable = conf.services.helios.enable or false;
          portKeyRpc = conf.services.helios.ports.rpc or "heliosRpc";
          executionRpcPortKey = conf.services.helios.ports.executionRpc or "rethHttp";
          dataDirName = conf.services.helios.dataDirName or "helios";
          network = conf.services.helios.network or "local";
          executionRpcUrl = conf.services.helios.executionRpcUrl or "";
          consensusRpcUrl = conf.services.helios.consensusRpcUrl or "";
          defaultConsensusRpcUrl =
            conf.services.helios.defaultConsensusRpcUrl or "https://www.lightclientdata.org";
          checkpoint = conf.services.helios.checkpoint or "";
          extraArgs = conf.services.helios.extraArgs or [ ];
          sources = conf.services.helios.sources or { };
          sourceKeys = normalizeSourceKeys (conf.services.helios.sources or { });
          defaultSource = conf.services.helios.defaultSource or "";
          sourceKinds = conf.services.helios.sourceKinds or { };
          readiness = {
            profile = conf.services.helios.readiness.profile or "fast";
            requireNotSyncing = conf.services.helios.readiness.requireNotSyncing or false;
            disallowSourceKinds = conf.services.helios.readiness.disallowSourceKinds or [ ];
          };
        };
      };

      operations = {
        enable = true;
        validateEnv.enable = true;
        testIsolation = {
          enable = conf.isolation.enable or true;
          slots =
            conf.isolation.slots or [
              conf.slots.default
            ];
          envs =
            let
              configured = conf.isolation.envs or [ ];
            in
            if configured == [ ] then envNames else configured;
          logsDir = conf.isolation.logsDir or "/tmp/${project.id}-isolation";
          keepLogsOnSuccess = conf.isolation.keepLogsOnSuccess or false;
          keepLogsOnFailure = conf.isolation.keepLogsOnFailure or true;
          maxParallel = conf.isolation.maxParallel or 4;
          runTaskId =
            conf.isolation.run.taskId or (
              let
                configuredApp = conf.isolation.run.app or "ci";
              in
              if configuredApp == "ci" then
                "task.ci"
              else
                throw "ERROR: isolation.run.taskId must be set when isolation.run.app is not 'ci'"
            );
          runApp = conf.isolation.run.app or "ci";
          runArgs = conf.isolation.run.args or [ "--summary" ];
          validateTaskId =
            conf.isolation.validate.taskId or (
              let
                configuredApp = conf.isolation.validate.app or "validate-env";
              in
              if configuredApp == "validate-env" then
                "task.ops.validate-env"
              else
                throw "ERROR: isolation.validate.taskId must be set when isolation.validate.app is not 'validate-env'"
            );
          validateApp = conf.isolation.validate.app or "validate-env";
          runEnv = conf.isolation.runEnv or { };
        };
        ports.enable = true;
        checkPorts.enable = true;
        health.enable = true;
        ready.enable = true;
      };

      tasks = {
        dev = mkCommandTask {
          id = "task.dev";
          appName = "dev";
          summary = "Start the dev workflow";
          description = ''
            Runs the project's dev workflow.

            Customize this command in nixfied/project/module.nix.
          '';
          tags = [
            "dev"
            "local"
          ];
          usage = [ "NIX_ENV=0 nix run .#dev" ];
          examples = [ "NIX_ENV=0 nix run .#dev" ];
          command = ''
            set -euo pipefail
            echo "INFO: starting dev workflow"
            echo "SKIP: dev command placeholder. Edit nixfied/project/module.nix to run your app."
          '';
        };

        build = mkCommandTask {
          id = "task.build";
          appName = "build";
          summary = "Build artifacts";
          description = ''
            Runs the project's build workflow.

            Customize this command in nixfied/project/module.nix.
          '';
          usage = [ "nix run .#build" ];
          command = ''
            set -euo pipefail
            echo "INFO: running build workflow"
            echo "SKIP: build command placeholder. Edit nixfied/project/module.nix."
          '';
        };

        check = mkCommandTask {
          id = "task.check";
          appName = "check";
          summary = "Run quality checks";
          description = ''
            Runs reusable Nix quality checks for the repository.
          '';
          usage = [
            "nix run .#check"
            "nix run .#check -- --full"
          ];
          examples = [ "nix run .#check -- --full" ];
          runner = {
            type = "derivation";
            package = nixChecksPkg;
            command = "nix-checks";
            workflowId = null;
          };
          contractArgs = nixChecksContractArgs;
        };

        format = mkCommandTask {
          id = "task.format";
          appName = "format";
          summary = "Format Nix files";
          usage = [ "nix run .#format" ];
          runtimeInputs = commonRuntimeInputs ++ [
            nixFormatterPkg
          ];
          postHooks = {
            "framework.nixfmt" = {
              command = lib.mkDefault ''
                set -euo pipefail
                find . -name '*.nix' -print0 | xargs -0 nixfmt --
                echo "OK: formatted nix files"
              '';
            };
          };
          command = ''
            set -euo pipefail
            echo "INFO: running format task"
          '';
        };

        test = mkCommandTask {
          id = "task.test";
          appName = "test";
          kind = "workflow";
          summary = "Run tests";
          description = "Run tests through the deterministic workflow executor.";
          usage = [ "nix run .#test" ];
          workflowId = "workflow.ci.full";
          contractArgs = [
            {
              name = "summary";
              kind = "flag";
              long = "--summary";
              description = "Print compact summary output.";
            }
            {
              name = "mode";
              kind = "option";
              long = "--mode";
              type = "enum";
              values = [
                "basic"
                "app"
                "env"
                "full"
              ];
              description = "Select workflow mode.";
            }
          ];
        };

        ci = mkCommandTask {
          id = "task.ci";
          appName = "ci";
          kind = "workflow";
          summary = "Run the CI pipeline";
          description = "Runs CI through workflow.ci.<mode> plans.";
          usage = [
            "nix run .#ci"
            "nix run .#ci -- --summary"
          ];
          workflowId = "workflow.ci.full";
          contractArgs = [
            {
              name = "summary";
              kind = "flag";
              long = "--summary";
              description = "Print compact summary output.";
            }
            {
              name = "mode";
              kind = "option";
              long = "--mode";
              type = "enum";
              values = [
                "basic"
                "app"
                "env"
                "full"
              ];
              description = "Select workflow mode.";
            }
            {
              name = "basic";
              kind = "flag";
              long = "--basic";
              description = "Alias for --mode basic.";
            }
            {
              name = "app";
              kind = "flag";
              long = "--app";
              description = "Alias for --mode app.";
            }
            {
              name = "env";
              kind = "flag";
              long = "--env";
              description = "Alias for --mode env.";
            }
            {
              name = "full";
              kind = "flag";
              long = "--full";
              description = "Alias for --mode full.";
            }
          ];
        };

        ci-quality =
          mkCommandTask {
            id = "task.ci.quality";
            appName = "ci-quality";
            kind = "ci-step";
            summary = "Quality checks";
            description = "Reusable Nix quality checks in full mode.";
            tags = [
              "ci"
              "quality"
            ];
            runner = {
              type = "derivation";
              package = nixChecksPkg;
              command = "nix-checks --mode full";
              workflowId = null;
            };
          }
          // {
            ui.app.expose = false;
          };

        ci-tests =
          mkCommandTask {
            id = "task.ci.tests";
            appName = "ci-tests";
            kind = "ci-step";
            summary = "Tests";
            description = "Test CI step.";
            tags = [
              "ci"
              "tests"
            ];
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
              mkdir -p "$artifacts_dir"
              touch "$artifacts_dir/tests.log"
              echo "OK: tests step complete"
            '';
          }
          // {
            ui.app.expose = false;
          };

        ci-system-quick =
          mkCommandTask {
            id = "task.ci.system-quick";
            appName = "ci-system-quick";
            kind = "ci-step";
            summary = "Quick system tests";
            description = "Optional system test gate.";
            tags = [
              "ci"
              "system"
            ];
            runtimeInputs = commonRuntimeInputs;
            passThroughEnv = defaultTaskPassThroughEnv ++ [ "API_KEY" ];
            allowSensitivePassThrough = true;
            command = ''
              set -euo pipefail
              if [ -z "''${API_KEY:-}" ]; then
                echo "SKIP: API_KEY not set"
                exit 0
              fi
              artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
              mkdir -p "$artifacts_dir"
              touch "$artifacts_dir/system-quick.log"
              echo "OK: quick system step complete"
            '';
          }
          // {
            ui.app.expose = false;
          };

        ci-nginx-proxy =
          mkCommandTask {
            id = "task.ci.nginx-proxy";
            appName = "ci-nginx-proxy";
            kind = "ci-step";
            summary = "Nginx proxy test";
            description = "Nginx proxy CI step.";
            tags = [
              "ci"
              "proxy"
            ];
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
              mkdir -p "$artifacts_dir"
              touch "$artifacts_dir/nginx-proxy.log"
              echo "OK: nginx proxy step complete"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-sleep-a =
          mkCommandTask {
            id = "task.test.parallel.sleep-a";
            appName = "test-parallel-sleep-a";
            kind = "internal";
            summary = "Parallel smoke unit A";
            description = "Sleeps for 1 second for workflow scheduler validation.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: parallel smoke sleep-a start"
              sleep 1
              echo "OK: parallel smoke sleep-a done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-sleep-b =
          mkCommandTask {
            id = "task.test.parallel.sleep-b";
            appName = "test-parallel-sleep-b";
            kind = "internal";
            summary = "Parallel smoke unit B";
            description = "Sleeps for 1 second for workflow scheduler validation.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: parallel smoke sleep-b start"
              sleep 1
              echo "OK: parallel smoke sleep-b done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-sleep-c =
          mkCommandTask {
            id = "task.test.parallel.sleep-c";
            appName = "test-parallel-sleep-c";
            kind = "internal";
            summary = "Parallel smoke dependent unit";
            description = "Sleeps for 1 second and depends on unit A in smoke workflow.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: parallel smoke sleep-c start"
              sleep 1
              echo "OK: parallel smoke sleep-c done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-sleep-d =
          mkCommandTask {
            id = "task.test.parallel.sleep-d";
            appName = "test-parallel-sleep-d";
            kind = "internal";
            summary = "Parallel smoke lock unit";
            description = "Sleeps for 1 second and shares lock with unit B.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: parallel smoke sleep-d start"
              sleep 1
              echo "OK: parallel smoke sleep-d done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-skip =
          mkCommandTask {
            id = "task.test.parallel.skip";
            appName = "test-parallel-skip";
            kind = "internal";
            summary = "Parallel smoke when-skip unit";
            description = "No-op task canceled by when.envPresent in smoke workflow.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "WARN: parallel smoke skip task should not run"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-fail =
          mkCommandTask {
            id = "task.test.parallel.fail";
            appName = "test-parallel-fail";
            kind = "internal";
            summary = "Parallel fail-fast trigger";
            description = "Fails intentionally for fail-fast workflow validation.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "ERROR: intentional fail-fast trigger"
              sleep 1
              exit 7
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-slow-a =
          mkCommandTask {
            id = "task.test.parallel.slow-a";
            appName = "test-parallel-slow-a";
            kind = "internal";
            summary = "Parallel fail-fast slow unit A";
            description = "Long-running unit that should be canceled by fail-fast.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: fail-fast slow-a start"
              sleep 10
              echo "OK: fail-fast slow-a done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        test-parallel-slow-b =
          mkCommandTask {
            id = "task.test.parallel.slow-b";
            appName = "test-parallel-slow-b";
            kind = "internal";
            summary = "Parallel fail-fast slow unit B";
            description = "Long-running unit that should be canceled by fail-fast.";
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              echo "INFO: fail-fast slow-b start"
              sleep 10
              echo "OK: fail-fast slow-b done"
            '';
          }
          // {
            ui.app.expose = false;
          };

        framework-test = mkCommandTask {
          id = "task.framework.test";
          appName = "framework::test";
          kind = "utility";
          summary = "Run framework validation in the model";
          description = ''
            Runs framework validation shards with configurable shard parallelism.
          '';
          runtimeInputs = [
            pkgs.bash
            pkgs.coreutils
            pkgs.findutils
            pkgs.gnugrep
            pkgs.gnused
            pkgs.nix
          ];
          usage = [
            "nix run .#framework::test"
            "nix run .#framework::test -- --summary"
            "nix run .#framework::test -- --mode env --summary-json /tmp/framework-summary.json"
          ];
          examples = [
            "nix run .#framework::test -- --list-shards"
            "nix run .#framework::test -- --shard flake-check"
            "nix run .#framework::test -- --shard isolation"
            "nix run .#framework::test -- --shard self-host"
          ];
          contractArgs = [
            {
              name = "summary";
              kind = "flag";
              long = "--summary";
              description = "Print compact summary output.";
            }
            {
              name = "summary-json";
              kind = "option";
              long = "--summary-json";
              type = "string";
              description = "Write summary JSON to a file.";
            }
            {
              name = "profile";
              kind = "option";
              long = "--profile";
              type = "enum";
              values = [ "ci" ];
              description = "Test profile to run (ci only).";
            }
            {
              name = "shard";
              kind = "option";
              long = "--shard";
              type = "string";
              values = [
                "flake-check"
                "help"
                "workflow-ci"
                "isolation"
                "self-host"
              ];
              description = "Run one shard only.";
            }
            {
              name = "max-parallel-shards";
              kind = "option";
              long = "--max-parallel-shards";
              type = "string";
              description = "Shard worker cap (positive integer) or 'auto' for all selected shards.";
            }
            {
              name = "serial";
              kind = "flag";
              long = "--serial";
              description = "Force serial shard execution.";
            }
            {
              name = "list-shards";
              kind = "flag";
              long = "--list-shards";
              description = "List available shards and exit.";
            }
            {
              name = "mode";
              kind = "option";
              long = "--mode";
              type = "enum";
              values = [
                "basic"
                "app"
                "env"
                "full"
              ];
              description = "CI workflow mode used by the workflow-ci shard.";
            }
            {
              name = "basic";
              kind = "flag";
              long = "--basic";
              description = "Alias for --mode basic.";
            }
            {
              name = "app";
              kind = "flag";
              long = "--app";
              description = "Alias for --mode app.";
            }
            {
              name = "env";
              kind = "flag";
              long = "--env";
              description = "Alias for --mode env.";
            }
            {
              name = "full";
              kind = "flag";
              long = "--full";
              description = "Alias for --mode full.";
            }
          ];
          command = ''
            set -euo pipefail

            ROOT="$(pwd -P)"
            PROFILE="ci"
            MODE="full"
            SHARD=""
            LIST_SHARDS=0
            SUMMARY=0
            SUMMARY_JSON=""
            MAX_PARALLEL_SHARDS_DEFAULT=${lib.escapeShellArg frameworkTestMaxParallelShards}
            MAX_PARALLEL_SHARDS="$MAX_PARALLEL_SHARDS_DEFAULT"
            SERIAL=0
            SHARDS=(
              "flake-check"
              "help"
              "workflow-ci"
              "isolation"
              "self-host"
            )
            EXECUTED=0
            FAILED_SHARDS=0
            EXIT_1_SHARDS=0
            CANCELED_SHARDS=0
            STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
            START_EPOCH="$(date +%s)"

            ${plainShellLogging { }}

            usage() {
              cat <<'EOF'
            Usage: nix run .#framework::test [-- --profile ci] [--mode <basic|app|env|full>] [--summary] [--summary-json <path>] [--shard <name>] [--max-parallel-shards <n|auto>] [--serial] [--list-shards]

            Shards:
              flake-check   Evaluate nix flake checks for the current project root.
              help          Validate generated help output.
              workflow-ci   Run the CI workflow surface in selected mode.
              isolation     Run isolation checks.
              self-host     Run a workflow that exercises framework entry points.
            EOF
            }

            print_shards() {
              local shard_name
              for shard_name in "''${SHARDS[@]}"; do
                printf '%s\n' "$shard_name"
              done
            }

            shard_exists() {
              local candidate="$1"
              local shard_name
              for shard_name in "''${SHARDS[@]}"; do
                if [ "$candidate" = "$shard_name" ]; then
                  return 0
                fi
              done
              return 1
            }

            write_summary_json() {
              local rc="$1"
              local finished_at duration
              local summary_dir summary_tmp
              finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
              duration="$(( $(date +%s) - START_EPOCH ))"
              summary_dir="$(dirname "$SUMMARY_JSON")"
              mkdir -p "$summary_dir"
              summary_tmp="$(mktemp "$SUMMARY_JSON.tmp.XXXXXX")"
              cat > "$summary_tmp" <<JSON
            {
              "profile": "$PROFILE",
              "mode": "$MODE",
              "shard": $(if [ -n "$SHARD" ]; then printf '"%s"' "$SHARD"; else printf 'null'; fi),
              "executed_shards": $EXECUTED,
              "failed_shards": $FAILED_SHARDS,
              "exit_1_shards": $EXIT_1_SHARDS,
              "canceled_shards": $CANCELED_SHARDS,
              "exit_code": $rc,
              "duration_seconds": $duration,
              "started_at": "$STARTED_AT",
              "finished_at": "$finished_at"
            }
            JSON
              mv "$summary_tmp" "$SUMMARY_JSON"
              log_info "wrote summary json path=$SUMMARY_JSON"
            }

            run_shard() {
              local shard_name="$1"
              shift
              log_info "running shard=$shard_name"
              if "$@"; then
                log_ok "shard passed name=$shard_name"
                return 0
              else
                local rc=$?
                log_error "shard failed name=$shard_name rc=$rc"
                return "$rc"
              fi
            }

            shard_flake_check() {
              nix flake check path:. --no-build
            }

            shard_help() {
              local help_stderr
              local rc
              help_stderr="$(mktemp)"
              if nix run path:.#help >/dev/null 2>"$help_stderr"; then
                rm -f "$help_stderr"
                return 0
              fi
              rc="$?"
              log_error "help shard command failed pwd=$(pwd -P) rc=$rc"
              cat "$help_stderr"
              rm -f "$help_stderr"
              return "$rc"
            }

            shard_workflow_ci() {
              if [ -z "''${NIXFIED_EXECUTOR_SELF:-}" ]; then
                log_error "NIXFIED_EXECUTOR_SELF is not set"
                return 3
              fi
              NIXFIED_CALLER_PWD="$PWD" "$NIXFIED_EXECUTOR_SELF" run-task task.ci --mode "$MODE" --summary
            }

            shard_isolation() {
              local -a isolation_args
              isolation_args=()

              if [ -z "''${NIXFIED_EXECUTOR_SELF:-}" ]; then
                log_error "NIXFIED_EXECUTOR_SELF is not set"
                return 3
              fi

              if [ "$SERIAL" -eq 1 ] || [ "''${CI:-}" = "1" ] || [ "''${CI:-}" = "true" ]; then
                isolation_args+=(--max-parallel 1)
              fi

              NIXFIED_CALLER_PWD="$PWD" "$NIXFIED_EXECUTOR_SELF" run-task task.ops.test-isolation "''${isolation_args[@]}"
            }

            shard_self_host() {
              if [ -z "''${NIXFIED_EXECUTOR_SELF:-}" ]; then
                log_error "NIXFIED_EXECUTOR_SELF is not set"
                return 3
              fi
              NIXFIED_CALLER_PWD="$PWD" "$NIXFIED_EXECUTOR_SELF" run-workflow workflow.test.framework.selfhost --summary
            }

            run_named_shard() {
              local shard_name="$1"
              case "$shard_name" in
                flake-check)
                  run_shard "$shard_name" shard_flake_check
                  ;;
                help)
                  run_shard "$shard_name" shard_help
                  ;;
                workflow-ci)
                  run_shard "$shard_name" shard_workflow_ci
                  ;;
                isolation)
                  run_shard "$shard_name" shard_isolation
                  ;;
                self-host)
                  run_shard "$shard_name" shard_self_host
                  ;;
                *)
                  log_error "unknown shard '$shard_name'"
                  return 2
                  ;;
              esac
            }

            run_named_shard_recorded() {
              local shard_name="$1"
              local rc=0
              if run_named_shard "$shard_name"; then
                EXECUTED="$((EXECUTED + 1))"
                return 0
              else
                rc="$?"
                FAILED_SHARDS="$((FAILED_SHARDS + 1))"
                if [ "$rc" -eq 1 ]; then
                  EXIT_1_SHARDS="$((EXIT_1_SHARDS + 1))"
                fi
              fi
              return "$rc"
            }

            resolve_parallel_workers() {
              local requested="$1"
              local shard_total="$2"
              local workers="$shard_total"

              if [ "$requested" != "auto" ]; then
                workers="$requested"
              fi

              if [ "$workers" -gt "$shard_total" ]; then
                workers="$shard_total"
              fi

              if [ "$workers" -lt 1 ]; then
                workers=1
              fi

              printf '%s' "$workers"
            }

            run_shards_parallel() {
              local requested_workers="$1"
              shift
              local shard_names=("''${@}")
              local shard_total="''${#shard_names[@]}"
              local workers
              local next_index=0
              local running_count=0
              local done_pid=""
              local done_shard=""
              local wait_rc=0
              local pid
              local failed=0
              local first_rc=1
              local failed_shard=""
              local pending_canceled=0
              local -A PID_TO_SHARD=()
              local -A CANCEL_REQUESTED=()

              if [ "$shard_total" -eq 0 ]; then
                return 0
              fi

              workers="$(resolve_parallel_workers "$requested_workers" "$shard_total")"
              if [ "$workers" -le 1 ]; then
                for shard_name in "''${shard_names[@]}"; do
                  run_named_shard_recorded "$shard_name" || return $?
                done
                return 0
              fi

              log_info "running shards parallel workers=$workers total=$shard_total"

              start_shard_worker() {
                local shard_name="$1"
                (
                  set +e
                  run_named_shard "$shard_name"
                ) &
                pid="$!"
                PID_TO_SHARD[$pid]="$shard_name"
                CANCEL_REQUESTED[$pid]=0
                running_count="$((running_count + 1))"
              }

              cancel_running_shards() {
                local active_pid
                for active_pid in "''${!PID_TO_SHARD[@]}"; do
                  CANCEL_REQUESTED[$active_pid]=1
                  kill -TERM "$active_pid" 2>/dev/null || true
                done

                sleep 5
                for active_pid in "''${!PID_TO_SHARD[@]}"; do
                  if kill -0 "$active_pid" 2>/dev/null; then
                    kill -KILL "$active_pid" 2>/dev/null || true
                  fi
                done
              }

              while [ "$running_count" -lt "$workers" ] && [ "$next_index" -lt "$shard_total" ]; do
                start_shard_worker "''${shard_names[$next_index]}"
                next_index="$((next_index + 1))"
              done

              while [ "''${#PID_TO_SHARD[@]}" -gt 0 ]; do
                if wait -n -p done_pid; then
                  wait_rc=0
                else
                  wait_rc="$?"
                fi

                done_shard="''${PID_TO_SHARD[$done_pid]:-}"
                if [ -z "$done_shard" ]; then
                  continue
                fi

                unset "PID_TO_SHARD[$done_pid]"
                running_count="$((running_count - 1))"

                if [ "''${CANCEL_REQUESTED[$done_pid]:-0}" = "1" ]; then
                  CANCELED_SHARDS="$((CANCELED_SHARDS + 1))"
                  continue
                fi

                if [ "$wait_rc" -eq 0 ]; then
                  EXECUTED="$((EXECUTED + 1))"
                else
                  FAILED_SHARDS="$((FAILED_SHARDS + 1))"
                  if [ "$wait_rc" -eq 1 ]; then
                    EXIT_1_SHARDS="$((EXIT_1_SHARDS + 1))"
                  fi
                  if [ "$failed" -eq 0 ]; then
                    first_rc="$wait_rc"
                    failed_shard="$done_shard"
                    pending_canceled="$((shard_total - next_index))"
                    CANCELED_SHARDS="$((CANCELED_SHARDS + pending_canceled))"
                    log_warn "framework::test fail-fast shard=$failed_shard rc=$first_rc pending_canceled=$pending_canceled running_canceled=''${#PID_TO_SHARD[@]}"
                    cancel_running_shards
                  fi
                  failed=1
                fi

                if [ "$failed" -eq 0 ]; then
                  while [ "$running_count" -lt "$workers" ] && [ "$next_index" -lt "$shard_total" ]; do
                    start_shard_worker "''${shard_names[$next_index]}"
                    next_index="$((next_index + 1))"
                  done
                fi
              done

              if [ "$failed" -eq 1 ]; then
                return "$first_rc"
              fi
              return 0
            }

            while [ "$#" -gt 0 ]; do
              case "$1" in
                --profile)
                  if [ "$#" -lt 2 ]; then
                    log_error "--profile requires a value"
                    exit 2
                  fi
                  PROFILE="$2"
                  shift 2
                  ;;
                --mode)
                  if [ "$#" -lt 2 ]; then
                    log_error "--mode requires a value"
                    exit 2
                  fi
                  MODE="$2"
                  shift 2
                  ;;
                --basic|--app|--env|--full)
                  MODE="''${1#--}"
                  shift
                  ;;
                --summary)
                  SUMMARY=1
                  shift
                  ;;
                --summary-json)
                  if [ "$#" -lt 2 ]; then
                    log_error "--summary-json requires a value"
                    exit 2
                  fi
                  SUMMARY_JSON="$2"
                  shift 2
                  ;;
                --shard)
                  if [ "$#" -lt 2 ]; then
                    log_error "--shard requires a value"
                    exit 2
                  fi
                  SHARD="$2"
                  shift 2
                  ;;
                --max-parallel-shards)
                  if [ "$#" -lt 2 ]; then
                    log_error "--max-parallel-shards requires a value"
                    exit 2
                  fi
                  MAX_PARALLEL_SHARDS="$2"
                  shift 2
                  ;;
                --serial)
                  SERIAL=1
                  shift
                  ;;
                --list-shards)
                  LIST_SHARDS=1
                  shift
                  ;;
                --help|-h)
                  usage
                  exit 0
                  ;;
                --)
                  shift
                  break
                  ;;
                *)
                  log_error "unknown option '$1'"
                  usage >&2
                  exit 2
                  ;;
              esac
            done

            if [ "$#" -gt 0 ]; then
              log_error "unexpected positional arguments: $*"
              exit 2
            fi

            case "$PROFILE" in
              ci)
                ;;
              full)
                log_error "profile 'full' is no longer supported; use --profile ci."
                exit 2
                ;;
              *)
                log_error "unknown profile '$PROFILE' (expected: ci)"
                exit 2
                ;;
            esac

            case "$MODE" in
              basic|app|env|full)
                ;;
              *)
                log_error "unknown mode '$MODE' (expected: basic|app|env|full)"
                exit 2
                ;;
            esac

            case "$MAX_PARALLEL_SHARDS" in
              auto)
                ;;
              *)
                if ! [[ "$MAX_PARALLEL_SHARDS" =~ ^[0-9]+$ ]]; then
                  log_error "invalid --max-parallel-shards '$MAX_PARALLEL_SHARDS' (expected: auto|positive-integer)"
                  exit 2
                fi
                if [ "$MAX_PARALLEL_SHARDS" -lt 1 ]; then
                  log_error "invalid --max-parallel-shards '$MAX_PARALLEL_SHARDS' (expected: auto|positive-integer)"
                  exit 2
                fi
                ;;
            esac

            if [ "$LIST_SHARDS" -eq 1 ]; then
              print_shards
              exit 0
            fi

            if [ -n "$SHARD" ] && ! shard_exists "$SHARD"; then
              log_error "unknown shard '$SHARD'"
              log_info "valid shards: $(print_shards | tr '\n' ' ')"
              exit 2
            fi

            cleanup() {
              local rc=$?
              if [ -n "$SUMMARY_JSON" ]; then
                write_summary_json "$rc"
              fi
              return "$rc"
            }
            trap cleanup EXIT

            selected_shards=()
            if [ -n "$SHARD" ]; then
              selected_shards+=("$SHARD")
            else
              selected_shards=("''${SHARDS[@]}")
            fi

            run_rc=0
            if [ "$SERIAL" -eq 1 ]; then
              log_info "running shards serial total=''${#selected_shards[@]}"
              for shard_name in "''${selected_shards[@]}"; do
                if run_named_shard_recorded "$shard_name"; then
                  :
                else
                  run_rc="$?"
                  break
                fi
              done
            elif [ "''${#selected_shards[@]}" -le 1 ]; then
              for shard_name in "''${selected_shards[@]}"; do
                if run_named_shard_recorded "$shard_name"; then
                  :
                else
                  run_rc="$?"
                  break
                fi
              done
            else
              if run_shards_parallel "$MAX_PARALLEL_SHARDS" "''${selected_shards[@]}"; then
                run_rc=0
              else
                run_rc="$?"
              fi
            fi

            if [ "$SUMMARY" -eq 1 ]; then
              log_info "summary profile=$PROFILE mode=$MODE executed_shards=$EXECUTED failed_shards=$FAILED_SHARDS exit_1_shards=$EXIT_1_SHARDS canceled_shards=$CANCELED_SHARDS"
            fi

            if [ "$run_rc" -ne 0 ]; then
              exit "$run_rc"
            fi

            log_ok "framework::test completed"
          '';
        };

        framework-install = mkCommandTask {
          id = "task.framework.install";
          appName = "framework::install";
          kind = "utility";
          summary = "Install thin or vendored wrapper flake";
          description = "Creates a thin wrapper flake by default, or a vendored wrapper with --vendor. Re-running with --vendor preserves nixfied/project and nixfied/local by default.";
          runtimeInputs = frameworkInstallRuntimeInputs;
          usage = [
            "nix run .#framework::install"
            "nix run .#framework::install -- --vendor"
            "nix run .#framework::install -- --vendor --target ."
            "nix run .#framework::install -- --vendor --upgrade --target ."
          ];
          contractArgs = frameworkInstallContractArgs;
          command = mkFrameworkInstallCommand { };
        };

        framework-upgrade = mkCommandTask {
          id = "task.framework.upgrade";
          appName = "framework::upgrade";
          kind = "utility";
          summary = "Upgrade vendored wrapper in-place";
          description = "Upgrades framework files while preserving nixfied/project and nixfied/local by default. Use --reset-project/--reset-local to overwrite those paths.";
          runtimeInputs = frameworkInstallRuntimeInputs;
          usage = [
            "nix run .#framework::upgrade -- --target ."
            "nix run .#framework::upgrade -- --target . --reset-project"
            "nix run .#framework::upgrade -- --target . --reset-local"
          ];
          contractArgs = frameworkUpgradeContractArgs;
          command = mkFrameworkInstallCommand {
            upgradeDefault = true;
          };
        };
      }
      // frameworkInstallPreset.tasks
      // frameworkTestPreset.tasks
      // frameworkSelfhostPreset.tasks;

      workflows = {
        ci-basic = {
          id = "workflow.ci.basic";
          summary = "Basic CI workflow";
          description = "Runs quality and tests.";
          mode = "ci";
          maxWorkers = 4;
          units = {
            quality = {
              taskId = "task.ci.quality";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            tests = {
              taskId = "task.ci.tests";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            parallel = true;
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
            ephemeral.enable = true;
          };
        };

        ci-app = {
          id = "workflow.ci.app";
          summary = "App CI workflow";
          description = "Basic workflow plus quick system checks.";
          mode = "ci";
          maxWorkers = 4;
          units = {
            quality = {
              taskId = "task.ci.quality";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            tests = {
              taskId = "task.ci.tests";
              needs = [ "quality" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            system-quick = {
              taskId = "task.ci.system-quick";
              needs = [ "tests" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ "API_KEY" ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            parallel = true;
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
            ephemeral.enable = true;
          };
        };

        ci-env = {
          id = "workflow.ci.env";
          summary = "Environment CI workflow";
          description = "App workflow plus nginx proxy checks.";
          mode = "ci";
          maxWorkers = 4;
          units = {
            quality = {
              taskId = "task.ci.quality";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            tests = {
              taskId = "task.ci.tests";
              needs = [ "quality" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            system-quick = {
              taskId = "task.ci.system-quick";
              needs = [ "tests" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ "API_KEY" ];
            };
            nginx-proxy = {
              taskId = "task.ci.nginx-proxy";
              needs = [ "system-quick" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            parallel = true;
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
            ephemeral.enable = true;
          };
        };

        ci-full = {
          id = "workflow.ci.full";
          summary = "Full CI workflow";
          description = "Runs quality/tests then system checks as deterministic stages.";
          mode = "ci";
          maxWorkers = 2;
          units = { };
          stages = [
            [
              "task.ci.quality"
              "task.ci.tests"
            ]
            [
              "task.ci.system-quick"
              "task.ci.nginx-proxy"
            ]
          ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            parallel = true;
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
            ephemeral.enable = true;
          };
        };

        test-parallel-smoke = {
          id = "workflow.test.parallel.smoke";
          summary = "Parallel runner smoke workflow";
          description = "Validates worker cap, dependency gating, locks, and when behavior.";
          mode = "custom";
          maxWorkers = 2;
          units = {
            alpha = {
              taskId = "task.test.parallel.sleep-a";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ "NIXFIED_PARALLEL_SMOKE" ];
              };
              skipIfMissingEnv = [ ];
            };
            beta = {
              taskId = "task.test.parallel.sleep-b";
              needs = [ ];
              locks = [ "smoke-lock" ];
              when = {
                envEquals = {
                  NIXFIED_PARALLEL_SMOKE = "1";
                };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            gamma = {
              taskId = "task.test.parallel.sleep-c";
              needs = [ "alpha" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            delta = {
              taskId = "task.test.parallel.sleep-d";
              needs = [ ];
              locks = [ "smoke-lock" ];
              when = {
                envEquals = { };
                envPresent = [ "NIXFIED_PARALLEL_SMOKE" ];
              };
              skipIfMissingEnv = [ ];
            };
            skip = {
              taskId = "task.test.parallel.skip";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ "NIXFIED_PARALLEL_SKIP" ];
              };
              skipIfMissingEnv = [ ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
          };
        };

        test-framework-selfhost = {
          id = "workflow.test.framework.selfhost";
          summary = "Framework self-host smoke workflow";
          description = "Runs internal self-host command through workflow orchestration.";
          mode = "custom";
          maxWorkers = 1;
          units = {
            main = {
              taskId = "task.test.framework.selfhost";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            parallel = false;
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
            ephemeral.enable = true;
          };
        };

        test-parallel-failfast = {
          id = "workflow.test.parallel.failfast";
          summary = "Parallel runner fail-fast workflow";
          description = "Validates fail-fast cancellation of running and pending units.";
          mode = "custom";
          maxWorkers = 3;
          units = {
            fail = {
              taskId = "task.test.parallel.fail";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            slow-a = {
              taskId = "task.test.parallel.slow-a";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            slow-b = {
              taskId = "task.test.parallel.slow-b";
              needs = [ ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
            after = {
              taskId = "task.test.parallel.sleep-c";
              needs = [ "slow-a" ];
              locks = [ ];
              when = {
                envEquals = { };
                envPresent = [ ];
              };
              skipIfMissingEnv = [ ];
            };
          };
          stages = [ ];
          preRun.tasks = [ "task.ops.ready" ];
          postRun = {
            tasks = [ "task.ops.health" ];
            alwaysRun = true;
          };
          artifacts = {
            root = "/tmp/ci-artifacts";
            keepOnSuccess = false;
            keepOnFailure = true;
            writeSummary = true;
          };
          execution = {
            failFast = true;
            lockPolicy = "exclusive";
            emitRegistryEvents = true;
          };
        };
      } // frameworkSelfhostPreset.workflows;
    };
  };
}
