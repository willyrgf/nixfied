{ project, ... }:

{
  # Example CI configuration (adapt as needed):
  # - enable postgres/nginx modules in conf.nix to use their hooks.

  commands = {
    ci = {
      description = "Run the CI pipeline";
      env = { "${project.envVar}" = "test"; };
      useDeps = true;
      script = ''
        echo "CI DSL is enabled. Edit nix/project/ci.nix to customize steps."
        exit 0
      '';
    };
  };

  ci = {
    enable = true;
    defaultMode = "basic";
    env = { "${project.envVar}" = "test"; };
    useDeps = true;
    setup = "";
    teardown = "";
    artifacts = {
      dir = "/tmp/ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = false;
    };
    modes = {
      basic = { steps = [ "quality" "tests" ]; };
      app = { steps = [ "quality" "tests" "system-quick" ]; };
      env = { steps = [ "quality" "tests" "system-quick" "nginx-proxy" ]; };
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
        requires = [ "nginx" ];
        run = ''
          run_hook NGINX_INIT
          run_hook NGINX_SITE_PROXY example.localhost 127.0.0.1 3000
          run_hook NGINX_START
          wait_http "http://localhost:8080"
        '';
      };
    };
  };
}
