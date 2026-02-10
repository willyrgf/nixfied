{ project, ... }:

{
  # Example (uncomment and adapt):
  # commands.test.script = ''
  #   eval "$(${SLOT_INFO})"
  #   run_hook POSTGRES_FULL_START_TEST
  #   ./run-tests
  # '';

  commands = {
    test = {
      description = "Run tests";
      api = {
        version = 1;
        summary = "Run tests";
        details = ''
          Runs the project's test workflow.

          Customize this command in nixfied/project/test.nix (start required services, run hooks, execute your test runner).
        '';
        usage = [ "nix run .#test" ];
        examples = [ "NIX_ENV=0 nix run .#test" ];
        env = [
          {
            name = project.envVar;
            description = "Environment name (set to test by default for this command)";
          }
          {
            name = project.slotVar;
            description = "Slot number (0-9)";
          }
        ];
        category = "core";
      };
      env = {
        "${project.envVar}" = "test";
      };
      useDeps = true;
      script = ''
        echo "Test command placeholder. Edit nixfied/project/test.nix."
        exit 0
      '';
    };
  };
}
