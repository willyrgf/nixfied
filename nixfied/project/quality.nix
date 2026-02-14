{
  commandLib,
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
      Also validates that discovery artifacts are current by default.

      Customize this command in nixfied/project/quality.nix.
    '';
    args = [
      {
        name = "--refresh-discovery";
        description = "Regenerate docs/repo-index.json and docs/repo-map.md before running checks.";
      }
    ];
    script = mkPlaceholderScript "Quality checks placeholder. Edit nixfied/project/quality.nix.";
  };
}
