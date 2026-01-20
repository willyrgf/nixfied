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
      env = { "${project.envVar}" = "dev"; };
      useDeps = true;
      script = ''
        echo "Dev command placeholder. Edit nix/project/dev.nix to run your app."
        exit 0
      '';
    };
  };
}
