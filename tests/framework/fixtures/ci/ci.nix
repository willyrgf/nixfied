{ project, ... }:

{
  commands.ci = {
    description = "CI DSL fixture";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    script = ''
      echo "CI DSL fixture command (overridden by DSL app)."
    '';
  };

  ci = {
    enable = true;
    defaultMode = "basic";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    setupActions = [ ];
    teardownActions = [
      {
        kind = "artifactTouch";
        artifact = "teardown.ok";
      }
    ];
    artifacts = {
      dir = ".ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = true;
    };
    modes = {
      basic = {
        steps = [
          "runs"
          "skip-missing"
          "when-false"
          "runs-second"
        ];
      };
      failure = {
        steps = [ "fail-with-cleanup" ];
      };
    };
    steps = {
      runs = {
        description = "Basic step runs";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "runs.ok";
          }
        ];
      };
      skip-missing = {
        description = "Skipped when env var missing";
        skipIfMissing = [ "CI_MISSING" ];
        actions = [
          {
            kind = "artifactTouch";
            artifact = "skip-missing.ok";
          }
        ];
      };
      when-false = {
        description = "Skipped when condition false";
        when = {
          envEquals = {
            "${project.envVar}" = "dev";
          };
        };
        actions = [
          {
            kind = "artifactTouch";
            artifact = "when.ok";
          }
        ];
      };
      runs-second = {
        description = "Second step runs";
        fixtures = {
          env = {
            FIXTURE_STEP_ENV = "from-fixture";
          };
        };
        actions = [
          {
            kind = "assertEnvEquals";
            name = "FIXTURE_STEP_ENV";
            value = "from-fixture";
          }
          {
            kind = "artifactTouch";
            artifact = "runs-second.ok";
          }
        ];
      };
      fail-with-cleanup = {
        description = "Cleanup runs on failure";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "fail.ran";
          }
          {
            kind = "fail";
            code = 1;
          }
        ];
        cleanupActions = [
          {
            kind = "artifactTouch";
            artifact = "fail.cleanup";
          }
        ];
      };
    };
  };
}
