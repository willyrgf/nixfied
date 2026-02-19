{
  commandLib,
  project,
}:

let
  inherit (commandLib)
    mkCommand
    mkPlaceholderCommand
    mkPlaceholderScript
    arg
    env
    failureProfiles
    ;
  ciModes = [
    "basic"
    "app"
    "env"
    "full"
  ];
  modeFlagDocs = map (mode: {
    name = "--${mode}";
    description = "Select ${mode} mode.";
  }) ciModes;
  modeFlagSpecs = map (
    mode:
    arg.flag {
      name = "mode_${mode}";
      long = "--${mode}";
    }
  ) ciModes;
in
{
  dev = mkPlaceholderCommand {
    name = "dev";
    description = "Start the dev workflow";
    details = ''
      Runs the project's dev workflow.

      Customize this command in nixfied/project/dev.nix (start services, run hooks, etc).
    '';
    examples = [ "NIX_ENV=0 nix run .#dev" ];
    envDefault = "dev";
    includeSlot = true;
    message = "Dev command placeholder. Edit nixfied/project/dev.nix to run your app.";
  };

  test = mkCommand {
    class = "batch-runner";
    name = "test";
    description = "Run tests";
    details = ''
      Runs tests through the shared CI staged pipeline definition.

      This keeps local test orchestration aligned with CI step contracts and lock behavior.
    '';
    usage = [
      "nix run .#test"
      "NIX_ENV=0 nix run .#test"
    ];
    examples = [ "nix run .#test" ];
    envDocs = [
      (commandLib.mkEnvDocProjectEnv "test")
      commandLib.mkEnvDocSlot
    ];
    env = {
      "${project.envVar}" = "test";
    };
    idempotent = false;
    script = ''
      set -euo pipefail
      nix run .#ci -- --mode full --summary
    '';
  };

  build = mkPlaceholderCommand {
    name = "build";
    description = "Build artifacts";
    details = ''
      Runs the project's build workflow (prod build).

      Customize this command in nixfied/project/prod.nix to build your artifacts (backend, frontend, etc).
    '';
    envDefault = "prod";
    message = "Build command placeholder. Edit nixfied/project/prod.nix.";
  };

  check = mkPlaceholderCommand {
    name = "check";
    description = "Run quality checks";
    details = ''
      Runs the project's quality checks (lint, typecheck, format checks, etc).
      Also validates that discovery artifacts are current by default.

      Customize this command in nixfied/project/quality.nix.
    '';
    args = [
      {
        name = "--refresh-discovery";
        description = "Regenerate docs/repo-index.json and docs/repo-map.md before running checks.";
      }
    ];
    message = "Quality checks placeholder. Edit nixfied/project/quality.nix.";
  };

  format = mkCommand {
    name = "format";
    description = "Format Nix files";
    details = ''
      Formats all *.nix files in the repository using nixfmt.

      Use this after making Nix changes.
    '';
    useDeps = false;
    script = ''
      find . -name '*.nix' -print0 | xargs -0 nixfmt --
    '';
  };

  ci = mkCommand {
    class = "batch-runner";
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
    ]
    ++ modeFlagDocs;
    contractArgs = [
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
    idempotent = false;
    script = mkPlaceholderScript "CI DSL is enabled. Edit nixfied/project/ci.nix to customize steps.";
  };

  ciConfig = {
    enable = true;
    defaultMode = "basic";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = true;
    setupActions = [ ];
    teardownActions = [ ];
    failureSignals = [ ];
    runsRoot = "/tmp/${project.id}-runs";
    useEphemeral = true;
    artifacts = {
      dir = "/tmp/ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = false;
    };
    parallel = {
      maxWorkers = 4;
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
      full = {
        parallel = {
          maxWorkers = 2;
        };
        stages = [
          [
            "quality"
            "tests"
          ]
          [
            "system-quick"
            "nginx-proxy"
          ]
        ];
      };
    };
    steps = {
      quality = {
        description = "Quality checks";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "quality.log";
          }
        ];
      };
      tests = {
        description = "Tests";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "tests.log";
          }
        ];
      };
      system-quick = {
        description = "Quick system tests";
        skipIfMissing = [ "API_KEY" ];
        actions = [
          {
            kind = "artifactTouch";
            artifact = "system-quick.log";
          }
        ];
      };
      nginx-proxy = {
        description = "Nginx proxy test";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "nginx-proxy.log";
          }
        ];
      };
    };
  };
}
