# Nixfied install surface: a thin wrapper over the CLI's scaffold command.
{ pkgs, cli }:
pkgs.writeShellApplication {
  name = "nixfied-install";
  text = ''
    exec "${cli}/bin/nixfied" install "$@"
  '';
}
