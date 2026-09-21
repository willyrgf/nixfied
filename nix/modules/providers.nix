# One lazy provider source for native evaluation and publication. No checked
# facade is imported here, so declaration-only evaluation cannot recurse into it.
{
  lib,
  pkgs,
  system,
}:
let
  adapters = import ../adapters/default.nix;
  declarations = [
    {
      kind = "argument";
      scope = "module-argument";
      name = "pkgs";
      description = "Nixpkgs package set for the selected target system.";
      valueDomain = "Native Nixpkgs package attribute set.";
      provider = "The supplying framework's pinned Nixpkgs and selected system.";
      usage = "{ pkgs, ... }: { nixfied.tasks.lint.invocation.tools = [ pkgs.git ]; }";
      binding = pkgs;
    }
    {
      kind = "argument";
      scope = "module-argument";
      name = "system";
      description = "Selected target system used by contextual defaults.";
      valueDomain = "Supported Nix system string.";
      provider = "The system passed to the native compiler evaluator.";
      usage = "{ system, ... }: { nixfied.target.system = system; }";
      binding = system;
    }
    {
      kind = "argument";
      scope = "module-argument";
      name = "adapters";
      description = "Importable native Nixfied adapter modules.";
      valueDomain = "Attribute set of native modules.";
      provider = "Adapter publication bindings from the supplying framework.";
      usage = "{ adapters, ... }: { imports = [ adapters.postgres ]; }";
      binding = builtins.listToAttrs (
        map (entry: {
          inherit (entry) name;
          value = entry.binding;
        }) adapters
      );
    }
    {
      kind = "argument";
      scope = "module-argument";
      name = "nixfiedLib";
      description = "Native task-composition helpers.";
      valueDomain = "Attribute set containing the seq function.";
      provider = "nix/lib/compose.nix from the supplying framework.";
      usage = ''{ nixfiedLib, ... }: { nixfied.tasks.all.steps = nixfiedLib.seq [ "lint" "test" ]; }'';
      binding = import ../lib/compose.nix { inherit lib; };
    }
  ];
in
{
  inherit declarations adapters;
  raw = builtins.listToAttrs (
    map (entry: {
      inherit (entry) name;
      value = entry.binding;
    }) declarations
  );
}
