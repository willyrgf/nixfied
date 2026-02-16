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
    setupActions = [ ];
    teardownActions = [ ];
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
        actions = [
          {
            kind = "artifactTouch";
            artifact = "passing.ok";
          }
        ];
      };
      skipped = {
        description = "A skipped step";
        skipIfMissing = [ "NONEXISTENT_VAR_FOR_TEST" ];
        actions = [
          {
            kind = "artifactTouch";
            artifact = "skipped.ok";
          }
        ];
      };
    };
  };
}
