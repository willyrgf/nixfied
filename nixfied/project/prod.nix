{
  commandLib,
  project,
  ...
}:

let
  inherit (commandLib) mkEnvDocProjectEnv mkPlaceholderScript mkProjectCommand;
in

{
  commands.build = mkProjectCommand {
    name = "build";
    description = "Build artifacts";
    details = ''
      Runs the project's build workflow (prod build).

      Customize this command in nixfied/project/prod.nix to build your artifacts (backend, frontend, etc).
    '';
    envDocs = [ (mkEnvDocProjectEnv "prod") ];
    env = {
      "${project.envVar}" = "prod";
    };
    script = mkPlaceholderScript "Build command placeholder. Edit nixfied/project/prod.nix.";
  };
}
