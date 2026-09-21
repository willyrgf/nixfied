# Nixfied install surface: a thin wrapper over the CLI's scaffold command.
{ pkgs, cli }:
let
  syntax = (import ../meta/command-default.nix { inherit (pkgs) lib; }).byName.install;
in
pkgs.writeShellApplication {
  name = "nixfied-install";
  text = ''
    exec "${cli}/bin/nixfied" ${syntax.name} "$@"
  '';
}
