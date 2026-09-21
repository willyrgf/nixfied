# Bootstrap raw native declarations, then expose checked provider projections.
{
  lib,
  pkgs,
  system,
}:
let
  providers = import ../modules/providers.nix { inherit lib pkgs system; };
  evaluated = lib.evalModules {
    specialArgs = providers.raw;
    modules = [ ../modules/default.nix ];
  };
  options = (import ./options.nix { inherit lib; }).collect evaluated.options;
  topics = import ../docs/topics.nix;
  targets =
    map (name: {
      kind = "topic";
      id = name;
    }) (builtins.attrNames topics)
    ++ map (entry: {
      kind = "command";
      id = entry.name;
    }) (import ./command-default.nix { inherit lib; }).commands
    ++ map (entry: {
      kind = "option";
      path = entry.loc;
    }) options;
  declarations = providers.declarations ++ providers.adapters;
  publication = import ./publications.nix { inherit lib; } { inherit declarations targets; };
in
builtins.seq options {
  inherit
    evaluated
    options
    targets
    declarations
    publication
    ;
  specialArgs = publication.project "argument" "module-argument";
}
