{
  commandLib,
  project,
  ...
}:

let
  inherit (commandLib)
    mkPlaceholderScript
    mkProjectBatchRunnerCommand
    arg
    env
    failureProfiles
    ;
  ciModes = [
    "basic"
    "app"
    "env"
  ];
  modeFlagDocs = map (mode: {
    name = "--${mode}";
    description = "Select ${mode} mode.";
  }) ciModes;
  modeFlagSpecs = map (mode: arg.flag {
    name = "mode_${mode}";
    long = "--${mode}";
  }) ciModes;
in

{
  commands.ci = mkProjectBatchRunnerCommand {
    name = "ci";
    description = "Run the CI pipeline";
    details = ''
      Runs the CI pipeline defined by the ci.modes and ci.steps configuration in this file.

      Customize steps, modes, artifacts, and hooks in nixfied/project/ci.nix.
    '';
    usage = [
      "nix run .#ci"
      "nix run .#ci -- --summary"
    ];
    examples = [ "nix run .#ci -- --summary" ];
    args = [
      {
        name = "--summary";
        description = "Print compact CI summary output.";
      }
      {
        name = "--bg";
        description = "Run CI in background via the run registry.";
      }
      {
        name = "--mode";
        description = "Select CI mode by name (value: <name>).";
      }
    ] ++ modeFlagDocs;
    contractArgs =
      [
        (arg.flag {
          name = "summary";
          long = "--summary";
        })
        (arg.flag {
          name = "bg";
          long = "--bg";
        })
        (arg.option {
          name = "mode";
          long = "--mode";
          type = "enum";
          values = ciModes;
        })
      ]
      ++ modeFlagSpecs;
    envDocs = [
      {
        name = "CI_ARTIFACTS_DIR";
        description = "Override artifact output directory.";
      }
      {
        name = "CI_ARTIFACTS_BASE";
        description = "Override artifacts root directory (absolute path).";
      }
    ];
    contractEnv = [
      (env.string {
        name = "CI_ARTIFACTS_DIR";
      })
      (env.typed {
        name = "CI_ARTIFACTS_BASE";
        type = "pathAbs";
      })
    ];
    failureCodes = failureProfiles.script;
    env = {
      "${project.envVar}" = "test";
    };
    script = mkPlaceholderScript "CI DSL is enabled. Edit nixfied/project/ci.nix to customize steps.";
  };

  ci = {
    enable = true;
    defaultMode = "basic";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = true;
    setup = "";
    teardown = "";
    failureSignals = [ ];
    runsRoot = "/tmp/${project.id}-runs";
    useEphemeral = true;
    artifacts = {
      dir = "/tmp/ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = false;
    };
    modes = {
      basic = {
        steps = [
          "quality"
          "tests"
        ];
      };
      app = {
        steps = [
          "quality"
          "tests"
          "system-quick"
        ];
      };
      env = {
        steps = [
          "quality"
          "tests"
          "system-quick"
          "nginx-proxy"
        ];
      };
    };
    steps = {
      quality = {
        description = "Quality checks";
        run = ''
          LOGFILE=$(artifact_path "quality.log")
          log_capture "$LOGFILE" -- "$BASH" -c 'echo "quality checks placeholder"'
        '';
      };
      tests = {
        description = "Tests";
        run = ''
          LOGFILE=$(artifact_path "tests.log")
          log_capture "$LOGFILE" -- "$BASH" -c 'echo "tests placeholder"'
        '';
      };
      system-quick = {
        description = "Quick system tests";
        skipIfMissing = [ "API_KEY" ];
        run = ''
          LOGFILE=$(artifact_path "system-quick.log")
          log_capture "$LOGFILE" -- "$BASH" -c 'echo "system tests placeholder"'
        '';
      };
      nginx-proxy = {
        description = "Nginx proxy test";
        run = ''
          LOGFILE=$(artifact_path "nginx-proxy.log")
          log_capture "$LOGFILE" -- "$BASH" -c 'echo "nginx proxy placeholder"'
        '';
      };
    };
  };
}
