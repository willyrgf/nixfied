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
    setup = ''
      mkdir -p .ci-artifacts
    '';
    teardown = ''
      touch "$(artifact_path "teardown.ok")"
    '';
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
        run = ''
          touch "$(artifact_path "runs.ok")"
        '';
      };
      skip-missing = {
        description = "Skipped when env var missing";
        skipIfMissing = [ "CI_MISSING" ];
        run = ''
          touch "$(artifact_path "skip-missing.ok")"
        '';
      };
      when-false = {
        description = "Skipped when condition false";
        when = "[ \"$PROJECT_ENV\" = \"dev\" ]";
        run = ''
          touch "$(artifact_path "when.ok")"
        '';
      };
      runs-second = {
        description = "Second step runs";
        fixtures = {
          env = {
            FIXTURE_STEP_ENV = "from-fixture";
          };
        };
        run = ''
          if [ "$FIXTURE_STEP_ENV" != "from-fixture" ]; then
            echo "fixture env missing" >&2
            exit 1
          fi
          touch "$(artifact_path "runs-second.ok")"
        '';
      };
      fail-with-cleanup = {
        description = "Cleanup runs on failure";
        run = ''
          touch "$(artifact_path "fail.ran")"
          exit 1
        '';
        cleanup = ''
          touch "$(artifact_path "fail.cleanup")"
        '';
      };
    };
  };
}
