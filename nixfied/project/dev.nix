{ project, ... }:

{
  # Example (uncomment and adapt):
  # commands.dev.script = ''
  #   eval "$(${SLOT_INFO})"
  #   run_hook POSTGRES_FULL_START
  #   start_service backend --wait-port "$BACKEND_PORT" -- ./start-backend
  #   start_service frontend --wait-http "http://localhost:$FRONTEND_PORT" -- ./start-frontend
  #   wait
  # '';

  commands = {
    dev = {
      description = "Start the dev workflow";
      api = {
        version = 1;
        summary = "Start the dev workflow";
        details = ''
          Runs the project's dev workflow.

          Customize this command in nixfied/project/dev.nix (start services, run hooks, etc).
        '';
        usage = [ "nix run .#dev" ];
        examples = [ "NIX_ENV=0 nix run .#dev" ];
        env = [
          {
            name = project.envVar;
            description = "Environment name (set to dev by default for this command)";
          }
          {
            name = project.slotVar;
            description = "Slot number (0-9)";
          }
        ];
        category = "core";
      };
      env = {
        "${project.envVar}" = "dev";
      };
      useDeps = true;
      script = ''
        echo "Dev command placeholder. Edit nixfied/project/dev.nix to run your app."
        exit 0
      '';
    };
  };
}
