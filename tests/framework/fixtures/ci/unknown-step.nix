{ project, ... }:

{
  ci = {
    enable = true;
    defaultMode = "broken";
    env = {
      "${project.envVar}" = "test";
    };
    useDeps = false;
    modes = {
      broken = {
        steps = [ "does-not-exist" ];
      };
    };
    steps = { };
  };
}
