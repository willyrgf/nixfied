# Nixfied install surface: a thin wrapper over the `nixfied` CLI's `install`
# command, which owns the scaffold templates. One source for the scaffold —
# the embedded-shell copy this file used to carry drifted from the CLI's
# during the composition rewrite, which is exactly the dual-source failure
# the rest of the design forbids.
{ pkgs, runtime }:
pkgs.writeShellApplication {
  name = "nixfied-install";
  text = ''
    exec "${runtime}/bin/nixfied" install "$@"
  '';
}
