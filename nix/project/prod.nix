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
      env = {
        "${project.envVar}" = "prod";
      };
      useDeps = true;
      script = ''
        echo "Build command placeholder. Edit nix/project/prod.nix."
        exit 0
      '';
    };
  };
}
