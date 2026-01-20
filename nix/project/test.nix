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
      env = {
        "${project.envVar}" = "test";
      };
      useDeps = true;
      script = ''
        echo "Test command placeholder. Edit nix/project/test.nix."
        exit 0
      '';
    };
  };
}
