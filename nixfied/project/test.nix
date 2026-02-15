{
  commandLib,
  project,
  ...
}:

let
  inherit (commandLib) mkEnvDocProjectEnv mkEnvDocSlot mkPlaceholderScript mkProjectTypedCommand;
in

{
  commands.test = mkProjectTypedCommand {
    name = "test";
    description = "Run tests";
    details = ''
      Runs the project's test workflow.

      Customize this command in nixfied/project/test.nix (start required services, run hooks, execute your test runner).
    '';
    examples = [ "NIX_ENV=0 nix run .#test" ];
    envDocs = [
      (mkEnvDocProjectEnv "test")
      mkEnvDocSlot
    ];
    env = {
      "${project.envVar}" = "test";
    };
    script = mkPlaceholderScript "Test command placeholder. Edit nixfied/project/test.nix.";
  };
}
