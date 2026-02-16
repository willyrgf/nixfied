{ project, ... }:

{
  ci = {
    enable = true;
    defaultMode = "success";
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
      keepOnSuccess = false;
    };
    modes = {
      success = {
        steps = [ "ok" ];
      };
      failure = {
        steps = [ "fail" ];
      };
    };
    steps = {
      ok = {
        description = "Success step";
        actions = [
          {
            kind = "artifactTouch";
            artifact = "ok.ok";
          }
        ];
      };
      fail = {
        description = "Failing step";
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
