{ project, ... }:

{
  # Example (uncomment and adapt):
  # commands.build.script = ''
  #   ./build-backend
  #   ./build-frontend
  # '';

  commands = {
    build = {
      description = "Build artifacts";
      api = {
        version = 1;
        summary = "Build artifacts";
        details = ''
          Runs the project's build workflow (prod build).

          Customize this command in nixfied/project/prod.nix to build your artifacts (backend, frontend, etc).
        '';
        usage = [ "nix run .#build" ];
        examples = [ "nix run .#build" ];
        env = [
          {
            name = project.envVar;
            description = "Environment name (set to prod by default for this command)";
          }
        ];
        category = "core";
      };
      env = {
        "${project.envVar}" = "prod";
      };
      useDeps = true;
      script = ''
        echo "Build command placeholder. Edit nixfied/project/prod.nix."
        exit 0
      '';
    };
  };
}
