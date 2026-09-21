{ lib }:
let
  structure = import ./default.nix { inherit lib; };
  inventory = import ./inventory.nix { inherit lib; } (
    builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
  );
  publications = import ../project-publications.nix { };
  checked = import ./syntax.nix { inherit lib; } {
    inherit structure inventory publications;
    declarations = import ./commands.nix { inherit lib structure; };
    topics = builtins.attrNames (import ../docs/topics.nix);
  };
in
assert lib.assertMsg (
  builtins.all (name: checked.byName ? ${name}) inventory.surface
  &&
    builtins.sort builtins.lessThan (
      map (entry: entry.command) (builtins.filter (entry: entry ? command) publications)
    ) == builtins.sort builtins.lessThan inventory.surface
) "Nixfied syntax: runtime declarations/publications differ from surface inventory";
checked // { runtimeCommands = inventory.surface; }
