# CI summary.json output test fixture
{ project, ... }:

{
  commands.ci = {
    description = "CI summary.json fixture";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    script = ''
      echo "CI summary.json fixture command."
    '';
  };

  ci = {
    enable = true;
    defaultMode = "check";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    setup = "";
    teardown = "";
    artifacts = {
      dir = ".ci-artifacts";
      keepOnFailure = true;
      keepOnSuccess = true;
    };
    modes = {
      check = {
        steps = [
          "passing"
          "skipped"
        ];
      };
    };
    steps = {
      passing = {
        description = "A passing step";
        run = ''
          echo "step passed"
        '';
      };
      skipped = {
        description = "A skipped step";
        skipIfMissing = [ "NONEXISTENT_VAR_FOR_TEST" ];
        run = ''
          echo "should not run"
        '';
      };
    };
  };
}
