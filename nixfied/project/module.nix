{ lib, pkgs, ... }:
let
  conf = import ./conf.nix { inherit pkgs; };
  project = conf.project;
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

  thinWrapperFlake = import ../install/wrapper-flake.nix {
    frameworkInput = "github:willyrgf/nixfied";
  };

  vendoredWrapperFlake = import ../install/wrapper-flake.nix {
    vendorPath = "./nixfied";
  };

  commonRuntimeInputs = [
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.gnugrep
  ];

  nixFormatterPkg = if pkgs ? nixfmt then pkgs.nixfmt else pkgs.nixfmt-rfc-style;

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
    contractArgs
    ++ lib.filter (arg: !(builtins.elem (argIdentity arg) existing)) loggingContractArgs;

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

            log_info() {
              printf 'INFO: %s\n' "$*"
            }

            log_error() {
              printf 'ERROR: %s\n' "$*" >&2
            }

            log_ok() {
              printf 'OK: %s\n' "$*"
            }

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
      contractArgs ? [ ],
      logging ? { },
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
        if workflowId == null then
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
        passThroughEnv = [
          "HOME"
          project.envVar
          project.slotVar
          "CI_ARTIFACTS_DIR"
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
          "API_KEY"
        ];
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
      };
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
        directories.base = conf.directories.base;
        ephemeral = {
          copyMode = conf.ephemeral.copyMode or "git-files";
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
        };
      };

      state = {
        registryRoot = conf.process.registryRoot;
        artifactsRoot = "/tmp/ci-artifacts";
      };

      tooling = {
        runtimePackages = conf.tooling.runtimePackages;
        devShellPackages = conf.tooling.devShellPackages;
        devShellHook = conf.tooling.devShellHook;
      };

      services = {
        postgres = {
          enable = conf.services.postgres.enable or false;
          database = conf.services.postgres.database or "app";
          portKey = conf.services.postgres.ports.primary or "postgres";
          sourceKeys = builtins.sort builtins.lessThan (builtins.attrNames (conf.services.postgres.sources or { }));
          defaultSource = conf.services.postgres.defaultSource or "";
        };

        nginx = {
          enable = conf.services.nginx.enable or false;
          portKeyHttp = conf.services.nginx.ports.http or "http";
          portKeyHttps = conf.services.nginx.ports.https or "https";
          sourceKeys = builtins.sort builtins.lessThan (builtins.attrNames (conf.services.nginx.sources or { }));
          defaultSource = conf.services.nginx.defaultSource or "";
        };

        minio = {
          enable = conf.services.minio.enable or false;
          portKeyApi = conf.services.minio.ports.api or "minioApi";
          portKeyConsole = conf.services.minio.ports.console or "minioConsole";
          sourceKeys = builtins.sort builtins.lessThan (builtins.attrNames (conf.services.minio.sources or { }));
          defaultSource = conf.services.minio.defaultSource or "";
        };

        reth = {
          enable = conf.services.reth.enable or false;
          portKeyHttp = conf.services.reth.ports.http or "rethHttp";
          portKeyWs = conf.services.reth.ports.ws or "rethWs";
          portKeyAuth = conf.services.reth.ports.auth or "rethAuth";
          sourceKeys = builtins.sort builtins.lessThan (builtins.attrNames (conf.services.reth.sources or { }));
          defaultSource = conf.services.reth.defaultSource or "";
        };

        helios = {
          enable = conf.services.helios.enable or false;
          portKeyRpc = conf.services.helios.ports.rpc or "heliosRpc";
          executionRpcPortKey = conf.services.helios.ports.executionRpc or "rethHttp";
          sourceKeys = builtins.sort builtins.lessThan (builtins.attrNames (conf.services.helios.sources or { }));
          defaultSource = conf.services.helios.defaultSource or "";
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
          runApp = conf.isolation.run.app or "ci";
          runArgs = conf.isolation.run.args or [ "--summary" ];
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
            Runs quality checks for the repository.

            Customize this command in nixfied/project/module.nix.
          '';
          usage = [ "nix run .#check" ];
          command = ''
            set -euo pipefail
            echo "INFO: running quality checks"
            echo "SKIP: quality checks placeholder. Edit nixfied/project/module.nix."
          '';
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
            description = "Quality CI step.";
            tags = [
              "ci"
              "quality"
            ];
            runtimeInputs = commonRuntimeInputs;
            command = ''
              set -euo pipefail
              artifacts_dir="''${CI_ARTIFACTS_DIR:-/tmp/ci-artifacts}"
              mkdir -p "$artifacts_dir"
              touch "$artifacts_dir/quality.log"
              echo "OK: quality step complete"
            '';
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
              artifacts_dir="''${CI_ARTIFACTS_DIR:-/tmp/ci-artifacts}"
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
            command = ''
              set -euo pipefail
              if [ -z "''${API_KEY:-}" ]; then
                echo "SKIP: API_KEY not set"
                exit 0
              fi
              artifacts_dir="''${CI_ARTIFACTS_DIR:-/tmp/ci-artifacts}"
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
              artifacts_dir="''${CI_ARTIFACTS_DIR:-/tmp/ci-artifacts}"
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

        test-framework-selfhost =
          mkCommandTask {
            id = "task.test.framework.selfhost";
            appName = "test-framework-selfhost";
            kind = "internal";
            summary = "Framework self-host smoke command";
            description = "Runs framework commands through dispatcher entry points.";
            runtimeInputs = commonRuntimeInputs ++ [
              pkgs.nix
            ];
            command = ''
              set -euo pipefail
              ROOT="$(pwd -P)"
              echo "INFO: self-host smoke start"
              nix run "path:$ROOT"#run-task -- task.dev > /dev/null
              nix run "path:$ROOT"#run-workflow -- workflow.ci.basic --summary > /dev/null
              echo "OK: self-host smoke complete"
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
                "workflow-test"
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
              "workflow-test"
              "workflow-ci"
              "isolation"
              "self-host"
            )
            EXECUTED=0
            STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
            START_EPOCH="$(date +%s)"

            log_info() {
              printf 'INFO: %s\n' "$*"
            }

            log_warn() {
              printf 'WARN: %s\n' "$*"
            }

            log_error() {
              printf 'ERROR: %s\n' "$*" >&2
            }

            log_ok() {
              printf 'OK: %s\n' "$*"
            }

            log_skip() {
              printf 'SKIP: %s\n' "$*"
            }

            usage() {
              cat <<'EOF'
            Usage: nix run .#framework::test [-- --profile ci] [--mode <basic|app|env|full>] [--summary] [--summary-json <path>] [--shard <name>] [--max-parallel-shards <n|auto>] [--serial] [--list-shards]

            Shards:
              flake-check   Run nix flake check for the current project root.
              help          Validate generated help output.
              workflow-test Run the test workflow surface.
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
              finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
              duration="$(( $(date +%s) - START_EPOCH ))"
              mkdir -p "$(dirname "$SUMMARY_JSON")"
              cat > "$SUMMARY_JSON" <<JSON
            {
              "profile": "$PROFILE",
              "mode": "$MODE",
              "shard": $(if [ -n "$SHARD" ]; then printf '"%s"' "$SHARD"; else printf 'null'; fi),
              "executed_shards": $EXECUTED,
              "exit_code": $rc,
              "duration_seconds": $duration,
              "started_at": "$STARTED_AT",
              "finished_at": "$finished_at"
            }
            JSON
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
              nix flake check path:.
            }

            shard_help() {
              nix run path:.#help >/dev/null
            }

            shard_workflow_test() {
              nix run path:.#test -- --summary
            }

            shard_workflow_ci() {
              nix run path:.#ci -- --mode "$MODE" --summary
            }

            shard_isolation() {
              nix run path:.#test-isolation
            }

            shard_self_host() {
              nix run path:.#run-workflow -- workflow.test.framework.selfhost --summary
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
                workflow-test)
                  run_shard "$shard_name" shard_workflow_test
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
              if run_named_shard "$shard_name"; then
                EXECUTED="$((EXECUTED + 1))"
                return 0
              fi
              return $?
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
              local temp_root
              local status_dir
              local semaphore_dir
              local semaphore_fifo
              local token_index
              local shard_name
              local status_file
              local worker_pids=()
              local worker_pid
              local rc
              local failed=0
              local first_rc=1

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

              temp_root="$(mktemp -d)"
              status_dir="$temp_root/status"
              semaphore_dir="$temp_root/semaphore"
              semaphore_fifo="$semaphore_dir/tokens.fifo"
              mkdir -p "$status_dir" "$semaphore_dir"

              mkfifo "$semaphore_fifo"
              exec 8<>"$semaphore_fifo"
              rm -f "$semaphore_fifo"

              token_index=0
              while [ "$token_index" -lt "$workers" ]; do
                printf 'token\n' >&8
                token_index="$((token_index + 1))"
              done

              for shard_name in "''${shard_names[@]}"; do
                status_file="$status_dir/$shard_name.rc"
                IFS= read -r -u 8 _
                (
                  set +e
                  run_named_shard "$shard_name"
                  rc="$?"
                  printf '%s\n' "$rc" > "$status_file"
                  printf 'token\n' >&8
                  exit 0
                ) &
                worker_pids+=("$!")
              done

              for worker_pid in "''${worker_pids[@]}"; do
                wait "$worker_pid" || true
              done

              exec 8>&-
              exec 8<&-

              for shard_name in "''${shard_names[@]}"; do
                status_file="$status_dir/$shard_name.rc"
                if [ ! -f "$status_file" ]; then
                  failed=1
                  first_rc=1
                  log_error "shard status missing name=$shard_name"
                  continue
                fi

                rc="$(cat "$status_file")"
                if [ "$rc" = "0" ]; then
                  EXECUTED="$((EXECUTED + 1))"
                else
                  if [ "$failed" -eq 0 ]; then
                    first_rc="$rc"
                  fi
                  failed=1
                fi
              done

              rm -rf "$temp_root"

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

            if [ "$SERIAL" -eq 1 ]; then
              log_info "running shards serial total=''${#selected_shards[@]}"
              for shard_name in "''${selected_shards[@]}"; do
                run_named_shard_recorded "$shard_name"
              done
            else
              run_shards_parallel "$MAX_PARALLEL_SHARDS" "''${selected_shards[@]}"
            fi

            if [ "$SUMMARY" -eq 1 ]; then
              log_info "summary profile=$PROFILE mode=$MODE executed_shards=$EXECUTED"
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
      };

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
            ephemeral.enable = null;
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
      };
    };
  };
}
