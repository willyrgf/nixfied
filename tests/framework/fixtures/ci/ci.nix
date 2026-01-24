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
          "requires-nginx"
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
      requires-nginx = {
        description = "Skipped when module disabled";
        requires = [ "nginx" ];
        run = ''
          touch "$(artifact_path "requires.ok")"
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
