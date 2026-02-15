{
  commandLib,
  project,
  ...
}:

let
  inherit (commandLib) mkEnvDocProjectEnv mkEnvDocSlot mkPlaceholderScript mkProjectTypedCommand;
in

{
  commands.dev = mkProjectTypedCommand {
    name = "dev";
    description = "Start the dev workflow";
    details = ''
      Runs the project's dev workflow.

      Customize this command in nixfied/project/dev.nix (start services, run hooks, etc).
    '';
    examples = [ "NIX_ENV=0 nix run .#dev" ];
    envDocs = [
      (mkEnvDocProjectEnv "dev")
      mkEnvDocSlot
    ];
    env = {
      "${project.envVar}" = "dev";
    };
    script = mkPlaceholderScript "Dev command placeholder. Edit nixfied/project/dev.nix to run your app.";
  };
}
