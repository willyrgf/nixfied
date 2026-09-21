# Native include-site routing; structure and syntax share one regeneration step.
{ lib, checked }:
let
  syntax = import ./command-default.nix { inherit lib; };
  projection = import ./syntax-project.nix {
    inherit lib;
    structure = checked;
  } syntax;
in
(import ./rust.nix { inherit lib; } checked)
// {
  "crates/nixfied-runtime/src/generated/commands.rs" = projection.rust syntax.runtimeCommands;
  "crates/nixfied-cli/src/generated/commands.rs" = projection.rust [ "install" ];
}
