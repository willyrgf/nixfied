{
  lib,
  pkgs,
  projectRoot,
  frameworkSourceRevision ? "unknown",
  ...
}:
let
  conf = import ./conf.nix { inherit pkgs; };
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
  nixChecksPkg = import ../lib/mkNixChecks.nix {
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
