{
  project,
  commandLib ? import ./lib/command.nix { inherit project; },
  ...
}:

let
  inherit (commandLib) mkProjectCommand;
in

{
  commands.format = mkProjectCommand {
    name = "format";
    description = "Format Nix files";
    details = ''
      Formats all *.nix files in the repository using nixfmt.

      Use this after making Nix changes.
    '';
    useDeps = false;
    script = ''
      find . -name '*.nix' -print0 | xargs -0 nixfmt --
    '';
  };
}
