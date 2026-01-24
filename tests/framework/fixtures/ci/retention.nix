{ project, ... }:

{
  ci = {
    enable = true;
    defaultMode = "success";
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
        run = ''
          touch "$(artifact_path "ok.ok")"
        '';
      };
      fail = {
        description = "Failing step";
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
