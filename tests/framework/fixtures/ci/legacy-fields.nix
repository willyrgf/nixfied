{ project, ... }:

{
  ci = {
    enable = true;
    defaultMode = "basic";
    env = {
      "${project.envVar}" = "test";
    };
    modes = {
      basic = {
        steps = [ "legacy-step" ];
      };
    };
    steps = {
      legacy-step = {
        description = "Legacy run string field should fail";
        run = ''
          echo "legacy"
        '';
      };
    };
  };
}
