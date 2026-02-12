{ project, ... }:

{
  # Example CI configuration (adapt as needed):
  # - enable postgres/nginx modules in conf.nix to use their hooks.

  commands = {
    ci = {
      description = "Run the CI pipeline";
      api = {
        version = 1;
        summary = "Run the CI pipeline";
        details = ''
          Runs the CI pipeline defined by the ci.modes and ci.steps configuration in this file.

          Customize steps, modes, artifacts, and hooks in nixfied/project/ci.nix.
        '';
        usage = [
          "nix run .#ci"
          "nix run .#ci -- --summary"
        ];
        examples = [ "nix run .#ci -- --summary" ];
        category = "core";
      };
      env = {
        "${project.envVar}" = "test";
      };
      useDeps = true;
      script = ''
        echo "CI DSL is enabled. Edit nixfied/project/ci.nix to customize steps."
        exit 0
      '';
    };
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
