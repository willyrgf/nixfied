{
  project,
  commandLib ? import ./lib/command.nix { inherit project; },
  ...
}:

let
  inherit (commandLib) mkPlaceholderScript mkProjectCommand;
in

{
  commands.check = mkProjectCommand {
    name = "check";
    description = "Run quality checks";
    details = ''
      Runs the project's quality checks (lint, typecheck, format checks, etc).

      Customize this command in nixfied/project/quality.nix.
    '';
    script = mkPlaceholderScript "Quality checks placeholder. Edit nixfied/project/quality.nix.";
  };
}
